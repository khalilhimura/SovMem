use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
use tantivy::schema::{IndexRecordOption, OwnedValue, Term};
use tantivy::tokenizer::{
    LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer, TokenStream,
};
use thiserror::Error;

use super::{
    open_current_index, open_p3_index_generation, open_verified_store_index, Direction,
    IndexMetadata, OpenedIndex, P3IndexEvidence, P3Profile, VerifiedGeneration, BODY_BOOST,
    TAGS_BOOST, TITLE_BOOST, TOKEN_LIMIT_BYTES,
};

const MAX_QUERY_BYTES: usize = 4_096;
const MAX_QUERY_TOKENS: usize = 64;
const MAX_TOP_K: usize = 62;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExpansionMode {
    None,
    OneHop,
}

pub struct QueryRequest {
    pub profile_path: PathBuf,
    pub bundle_root: PathBuf,
    pub index_root: PathBuf,
    pub query: String,
    pub top_k: usize,
    pub expansion: ExpansionMode,
}

pub struct StoreQueryRequest {
    pub profile_path: PathBuf,
    pub store_root: PathBuf,
    pub query: String,
    pub top_k: usize,
    pub expansion: ExpansionMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreQueryResult {
    pub result: QueryResult,
    pub evidence: P3IndexEvidence,
    pub generation: String,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct QueryResult {
    pub top_k: usize,
    pub expansion: ExpansionMode,
    pub bundle_hash: String,
    pub results: Vec<QueryHit>,
    pub expanded: Vec<ExpandedHit>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct QueryHit {
    pub rank: usize,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub core_type: String,
    pub layer: String,
    pub provenance: String,
    pub score: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExpandedHit {
    pub rank: usize,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub core_type: String,
    pub layer: String,
    pub provenance: String,
    pub via_id: String,
    pub direction: Direction,
    pub relation: String,
}

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("index could not be opened")]
    Index(#[from] super::IndexError),
    #[error("query input is invalid")]
    Invalid,
    #[error("query produced no terms")]
    Empty,
    #[error("query produced too many terms")]
    TooManyTokens,
    #[error("query execution failed")]
    Execute,
}

impl QueryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Index(error) => error.code(),
            Self::Invalid => "query.invalid",
            Self::Empty => "query.empty",
            Self::TooManyTokens => "query.too_many_tokens",
            Self::Execute => "query.execute_failed",
        }
    }
}

struct ScoredHit {
    score: f32,
    metadata: IndexMetadata,
}

pub fn tokenize_query(query: &str) -> Vec<String> {
    let mut analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(TOKEN_LIMIT_BYTES))
        .filter(LowerCaser)
        .build();
    let mut stream = analyzer.token_stream(query);
    let mut tokens = Vec::new();
    while stream.advance() {
        tokens.push(stream.token().text.clone());
    }
    tokens
}

pub fn validate_query(query: &str, top_k: usize) -> Result<Vec<String>, QueryError> {
    if !(1..=MAX_TOP_K).contains(&top_k)
        || query.len() > MAX_QUERY_BYTES
        || query.chars().any(char::is_control)
    {
        return Err(QueryError::Invalid);
    }
    if query.trim().is_empty() {
        return Err(QueryError::Empty);
    }

    let tokens = tokenize_query(query);
    if tokens.is_empty() {
        return Err(QueryError::Empty);
    }
    if tokens.len() > MAX_QUERY_TOKENS {
        return Err(QueryError::TooManyTokens);
    }
    Ok(tokens)
}

pub fn query_index(request: QueryRequest) -> Result<QueryResult, QueryError> {
    let tokens = validate_query(&request.query, request.top_k)?;
    let opened = open_current_index(
        &request.profile_path,
        &request.bundle_root,
        &request.index_root,
    )?;
    execute_query(&opened, &tokens, request.top_k, request.expansion)
}

pub fn query_p3_generation(
    profile: &P3Profile,
    generation: &VerifiedGeneration,
    index_root: &std::path::Path,
    query: String,
    top_k: usize,
    expansion: ExpansionMode,
) -> Result<QueryResult, QueryError> {
    let (opened, _) = open_p3_index_generation(profile, generation, index_root)?;
    search_opened_index(&opened, &query, top_k, expansion)
}

pub fn query_store(request: StoreQueryRequest) -> Result<StoreQueryResult, QueryError> {
    let (opened, evidence, generation, manifest_hash) =
        open_verified_store_index(&request.profile_path, &request.store_root)?;
    let result = search_opened_index(&opened, &request.query, request.top_k, request.expansion)?;
    Ok(StoreQueryResult {
        result,
        evidence,
        generation,
        manifest_hash,
    })
}

#[allow(dead_code)]
pub(crate) fn search_opened_index(
    opened: &OpenedIndex,
    query: &str,
    top_k: usize,
    expansion: ExpansionMode,
) -> Result<QueryResult, QueryError> {
    let tokens = validate_query(query, top_k)?;
    execute_query(opened, &tokens, top_k, expansion)
}

fn execute_query(
    opened: &OpenedIndex,
    tokens: &[String],
    top_k: usize,
    expansion: ExpansionMode,
) -> Result<QueryResult, QueryError> {
    let query = BooleanQuery::new(term_clauses(opened, tokens));
    let searcher = opened.reader.searcher();
    let matches = searcher
        .search(&query, &TopDocs::with_limit(opened.manifest.documents))
        .map_err(|_| QueryError::Execute)?;

    let mut seen_ids = BTreeSet::new();
    let mut scored = Vec::with_capacity(matches.len());
    for (score, address) in matches {
        if !score.is_finite() {
            return Err(QueryError::Execute);
        }
        let document: tantivy::TantivyDocument =
            searcher.doc(address).map_err(|_| QueryError::Execute)?;
        let id = stored_id(&document, opened)?;
        if !seen_ids.insert(id.clone()) {
            return Err(QueryError::Execute);
        }
        let metadata = opened
            .metadata
            .get(&id)
            .cloned()
            .ok_or(QueryError::Execute)?;
        scored.push(ScoredHit { score, metadata });
    }

    scored.sort_by(|left, right| {
        right.score.total_cmp(&left.score).then_with(|| {
            left.metadata
                .id
                .as_bytes()
                .cmp(right.metadata.id.as_bytes())
        })
    });
    scored.truncate(top_k);

    let results: Vec<_> = scored
        .into_iter()
        .enumerate()
        .map(|(position, hit)| QueryHit {
            rank: position + 1,
            id: hit.metadata.id,
            title: hit.metadata.title,
            core_type: hit.metadata.core_type,
            layer: hit.metadata.layer,
            provenance: hit.metadata.provenance,
            score: format!("{:.6}", hit.score),
        })
        .collect();
    let expanded = match expansion {
        ExpansionMode::None => Vec::new(),
        ExpansionMode::OneHop => {
            let seed_ids = results.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>();
            opened
                .graph
                .adjacency()
                .expand_one_hop(&seed_ids)
                .into_iter()
                .enumerate()
                .map(|(position, witness)| {
                    let metadata = opened
                        .metadata
                        .get(&witness.id)
                        .ok_or(QueryError::Execute)?;
                    Ok(ExpandedHit {
                        rank: position + 1,
                        id: metadata.id.clone(),
                        title: metadata.title.clone(),
                        core_type: metadata.core_type.clone(),
                        layer: metadata.layer.clone(),
                        provenance: metadata.provenance.clone(),
                        via_id: witness.via_id,
                        direction: witness.direction,
                        relation: witness.relation,
                    })
                })
                .collect::<Result<Vec<_>, QueryError>>()?
        }
    };

    Ok(QueryResult {
        top_k,
        expansion,
        bundle_hash: opened.manifest.bundle_hash.clone(),
        results,
        expanded,
    })
}

fn term_clauses(opened: &OpenedIndex, tokens: &[String]) -> Vec<(Occur, Box<dyn Query>)> {
    let mut clauses = Vec::with_capacity(tokens.len() * 3);
    for token in tokens {
        for (field, boost) in [
            (opened.fields.title, TITLE_BOOST),
            (opened.fields.tags, TAGS_BOOST),
            (opened.fields.body, BODY_BOOST),
        ] {
            let term = Term::from_field_text(field, token);
            let query = TermQuery::new(term, IndexRecordOption::WithFreqs);
            clauses.push((
                Occur::Should,
                Box::new(BoostQuery::new(Box::new(query), boost)) as Box<dyn Query>,
            ));
        }
    }
    clauses
}

fn stored_id(
    document: &tantivy::TantivyDocument,
    opened: &OpenedIndex,
) -> Result<String, QueryError> {
    let mut values = document.get_all(opened.fields.id);
    let value = values.next().ok_or(QueryError::Execute)?;
    if values.next().is_some() {
        return Err(QueryError::Execute);
    }
    match OwnedValue::from(value) {
        OwnedValue::Str(id) => Ok(id),
        _ => Err(QueryError::Execute),
    }
}
