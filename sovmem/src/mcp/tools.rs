use serde_json::{json, Value};
use std::path::Path;

use crate::profile::{
    inspect_store, intake_prefix, load_p3_profile, parse_source_document, query_store, read_intake,
    submit_proposal, verify_generation, CanonicalHead, Confidence, ExpansionMode, ProposalError,
    ProposalIntent, ProposalOperation, ProposalSubmission, QueryError, SourceDocument, StorePaths,
    StoreQueryRequest, VerificationLevel,
};

pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Result of a tool execution.
pub enum ToolResult {
    Success { text: String, structured: Value },
    Error { code: &'static str, message: String },
}

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "memory_search",
            description: "Search the SovMem knowledge base for documents matching a query",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (1-200 characters)"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Maximum number of results (default 5, max 20)",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 20
                    },
                    "expand": {
                        "type": "string",
                        "description": "Graph expansion mode: \"none\" or \"one-hop\"",
                        "enum": ["none", "one-hop"],
                        "default": "none"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "memory_get",
            description: "Retrieve a single canonical document by its exact ID",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Canonical document ID (e.g. ssa-0/04-Research/topic.md)"
                    },
                    "include_body": {
                        "type": "boolean",
                        "description": "Include document body text (max 4000 chars)",
                        "default": false
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "memory_propose",
            description: "Submit an untrusted proposal to the inbox for human review",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Source identifier (e.g. src:agent:claude-code)"
                    },
                    "operation": {
                        "type": "string",
                        "description": "Proposal operation: \"Create\" or \"Replace\"",
                        "enum": ["Create", "Replace"]
                    },
                    "intent": {
                        "type": "string",
                        "description": "Proposal intent: \"Accept\" or \"Rewrite\"",
                        "enum": ["Accept", "Rewrite"]
                    },
                    "target_id": {
                        "type": "string",
                        "description": "Canonical document ID to replace, or new path for Create"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Relative vault path (e.g. 04-Research/topic.md)"
                    },
                    "candidate_body": {
                        "type": "string",
                        "description": "Full Markdown body of the proposed document"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "Pre-draft confidence estimate (0.0-1.0, optional)",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "evaluator": {
                        "type": "object",
                        "description": "Evaluator block (optional)",
                        "properties": {
                            "agent": { "type": "string" },
                            "recommendation": { "type": "string" },
                            "reasons": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        }
                    }
                },
                "required": ["source", "operation", "intent", "target_id", "target_path", "candidate_body"]
            }),
        },
        ToolDefinition {
            name: "memory_status",
            description: "Get read-only status of the SovMem store, index, and intake queue",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count lines in the ledger file as a proxy for verdict count.
fn count_verdicts(paths: &StorePaths) -> usize {
    std::fs::read(&paths.ledger)
        .ok()
        .map(|bytes| bytes.iter().filter(|&&b| b == b'\n').count())
        .unwrap_or(0)
}

/// Count inbox submission directories.
fn count_inbox_submissions(paths: &StorePaths) -> usize {
    std::fs::read_dir(&paths.inbox)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| {
                    e.ok().filter(|entry| {
                        let name = entry.file_name();
                        let name = name.to_str().unwrap_or("");
                        name.starts_with("sub-")
                    })
                })
                .count()
        })
        .unwrap_or(0)
}

/// Build a StoreQueryRequest from the common parameters.
fn build_query_request(
    profile_path: &Path,
    store_root: &Path,
    query: &str,
    top_k: usize,
    expand: &str,
) -> StoreQueryRequest {
    let expansion = match expand {
        "one-hop" => ExpansionMode::OneHop,
        _ => ExpansionMode::None,
    };
    StoreQueryRequest {
        profile_path: profile_path.to_path_buf(),
        store_root: store_root.to_path_buf(),
        query: query.to_string(),
        top_k,
        expansion,
    }
}

// ---------------------------------------------------------------------------
// memory_status
// ---------------------------------------------------------------------------

pub fn handle_memory_status(profile_path: &Path, store_root: &Path) -> ToolResult {
    let profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult::Error {
                code: "store.unreadable",
                message: format!("Profile could not be loaded: {}", e.code()),
            }
        }
    };
    let snapshot = match inspect_store(&profile, store_root) {
        Ok(s) => s,
        Err(_) => {
            return ToolResult::Error {
                code: "store.unreadable",
                message: "Store could not be inspected".to_string(),
            }
        }
    };

    // Count intake states
    let intake = read_intake(&snapshot.paths).ok();
    let (staged, ruled) = match &intake {
        Some(view) => {
            let head = view.head();
            let prefix = intake_prefix(view, head).ok();
            match prefix {
                Some(prefix) => (prefix.staged.len(), prefix.rejected.len()),
                None => (0, 0),
            }
        }
        None => (0, 0),
    };
    let submissions = count_inbox_submissions(&snapshot.paths);

    let sealed = snapshot.sealed;
    let (doc_count, generation_str) = match &snapshot.head {
        CanonicalHead::Seed => (0usize, Some("seed".to_string())),
        CanonicalHead::Generation(gen_id) => {
            let gen_root = snapshot.paths.canon.join("generations").join(gen_id);
            match verify_generation(&profile, &gen_root, VerificationLevel::Three) {
                Ok(verified_gen) => {
                    let count = verified_gen.manifest.documents.len();
                    let gen_str = verified_gen.manifest.generation_id.clone();
                    (count, Some(gen_str))
                }
                Err(_) => (0usize, Some(gen_id.clone())),
            }
        }
    };

    let verdicts = count_verdicts(&snapshot.paths);

    // Read last line from the ledger for a timestamp
    let last_verdict_at = std::fs::read_to_string(&snapshot.paths.ledger)
        .ok()
        .and_then(|content| {
            content
                .lines()
                .last()
                .and_then(|line| serde_json::from_str::<Value>(line).ok())
                .and_then(|v| {
                    v.get("occurred_at")
                        .and_then(|o| o.as_str().map(String::from))
                })
        });

    ToolResult::Success {
        text: format!(
            "Store state: {} | {} documents | Verdicts: {}",
            if sealed { "sealed" } else { "unsealed" },
            doc_count,
            verdicts,
        ),
        structured: json!({
            "store": {
                "state": if sealed { "sealed" } else { "unsealed" },
                "canonical_documents": doc_count,
                "canonical_generation": generation_str,
                "level_4_verified": false,
            },
            "intake": {
                "pending_submissions": submissions,
                "staged_proposals": staged,
                "ruled_proposals": ruled,
            },
            "ledger": {
                "total_verdicts": verdicts,
                "last_verdict_at": last_verdict_at,
            },
        }),
    }
}

// ---------------------------------------------------------------------------
// memory_search
// ---------------------------------------------------------------------------

pub fn handle_memory_search(
    profile_path: &Path,
    store_root: &Path,
    query: &str,
    top_k: usize,
    expand: &str,
) -> ToolResult {
    // Validate inputs
    if query.trim().is_empty() || query.len() > 200 {
        return ToolResult::Error {
            code: "query.invalid",
            message: "Query must be between 1 and 200 characters".to_string(),
        };
    }
    let top_k = top_k.clamp(1, 20);

    let request = build_query_request(profile_path, store_root, query, top_k, expand);

    match query_store(request) {
        Ok(result) => {
            let results: Vec<Value> = result
                .result
                .results
                .into_iter()
                .map(|hit| {
                    json!({
                        "id": hit.id,
                        "score": hit.score.parse::<f64>().unwrap_or(0.0),
                        "title": hit.title,
                        "type": hit.core_type,
                        "layer": hit.layer,
                        "provenance": hit.provenance,
                    })
                })
                .collect();

            let total_hits = results.len();

            ToolResult::Success {
                text: format!("Found {} result(s) for query '{}'", total_hits, query),
                structured: json!({
                    "results": results,
                    "total_hits": total_hits,
                    "generation_id": result.generation,
                }),
            }
        }
        Err(e) => ToolResult::Error {
            code: match e {
                QueryError::Empty => "query.invalid",
                QueryError::Invalid | QueryError::TooManyTokens => "query.invalid",
                QueryError::Index(_) => "index.missing",
                _ => "server.internal",
            },
            message: format!("Search failed: {}", e.code()),
        },
    }
}

// ---------------------------------------------------------------------------
// memory_get
// ---------------------------------------------------------------------------

pub fn handle_memory_get(
    profile_path: &Path,
    store_root: &Path,
    id: &str,
    include_body: bool,
) -> ToolResult {
    if id.trim().is_empty() {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID must not be empty".to_string(),
        };
    }

    // Reject path traversal patterns
    if id.contains("..") || id.contains("//") || id.starts_with('/') {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID contains invalid characters".to_string(),
        };
    }

    let profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(_) => {
            return ToolResult::Error {
                code: "store.unreadable",
                message: "Profile could not be loaded".to_string(),
            }
        }
    };

    // Validate the ID is a well-formed canonical document ID
    if !id.ends_with(".md") {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID must end with .md".to_string(),
        };
    }

    // Look up the document in the current generation's canon manifest
    let snapshot = match inspect_store(&profile, store_root) {
        Ok(s) => s,
        Err(_) => {
            return ToolResult::Error {
                code: "store.unreadable",
                message: "Store could not be inspected".to_string(),
            }
        }
    };

    let generation = match &snapshot.head {
        CanonicalHead::Generation(gen_id) => {
            let gen_root = snapshot.paths.canon.join("generations").join(gen_id);
            match verify_generation(&profile, &gen_root, VerificationLevel::Three) {
                Ok(verified_gen) => verified_gen,
                Err(_) => {
                    return ToolResult::Error {
                        code: "store.unreadable",
                        message: "Generation could not be verified".to_string(),
                    }
                }
            }
        }
        CanonicalHead::Seed => {
            return ToolResult::Error {
                code: "document.not_found",
                message: "No canon generation found (store is in seed state)".to_string(),
            }
        }
    };

    // Find the document in the canon manifest
    let doc = match generation.manifest.documents.iter().find(|d| d.id == id) {
        Some(d) => d,
        None => {
            return ToolResult::Error {
                code: "document.not_found",
                message: "Document was not found".to_string(),
            }
        }
    };

    // Read the actual file from the canon bundle
    let canon_path = generation.root.join(&doc.output_path);
    let body = if include_body {
        match std::fs::read_to_string(&canon_path) {
            Ok(b) => {
                let truncated: String = b.chars().take(4000).collect();
                Some(truncated)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Parse frontmatter for metadata by constructing a SourceDocument
    let bytes = std::fs::read(&canon_path).ok();
    let frontmatter = bytes.and_then(|b| {
        let source_doc = SourceDocument {
            source_path: doc.output_path.clone(),
            bytes: b,
            source_hash: doc.hash.clone(),
            created: String::new(),
            updated: String::new(),
        };
        parse_source_document(source_doc).ok()
    });

    // Extract metadata fields from parsed frontmatter
    let (title, core_type, layer, tags, provenance, source, created, updated) = match &frontmatter {
        Some(parsed) => {
            let meta = &parsed.metadata;
            let t = meta
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ct = meta
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let l = meta
                .get("layer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tg = meta
                .get("tags")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let p = meta
                .get("provenance")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let s = meta
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let c = meta
                .get("created")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let u = meta
                .get("updated")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (t, ct, l, tg, p, s, c, u)
        }
        None => (
            String::new(),
            String::new(),
            String::new(),
            Vec::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };

    ToolResult::Success {
        text: format!(
            "Document: {}\nType: {} | Layer: {}\nProvenance: {}",
            title, core_type, layer, provenance
        ),
        structured: json!({
            "id": doc.id,
            "title": title,
            "type": core_type,
            "layer": layer,
            "tags": tags,
            "provenance": provenance,
            "source": source,
            "created": created,
            "updated": updated,
            "body": body,
            "generation_id": generation.manifest.generation_id,
        }),
    }
}

// ---------------------------------------------------------------------------
// memory_propose
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn handle_memory_propose(
    profile_path: &Path,
    store_root: &Path,
    source: &str,
    operation: &str,
    intent: &str,
    target_id: &str,
    target_path: &str,
    candidate_body: &str,
    confidence: Option<f64>,
    evaluator: Option<Value>,
) -> ToolResult {
    // Validate required fields
    if source.trim().is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "Source is required".to_string(),
        };
    }
    if target_id.trim().is_empty() || target_path.trim().is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "target_id and target_path are required".to_string(),
        };
    }
    if candidate_body.is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "candidate_body must not be empty".to_string(),
        };
    }

    // Size limit
    if candidate_body.len() > 100_000 {
        return ToolResult::Error {
            code: "proposal.size_exceeded",
            message: "Candidate body exceeds maximum size of 100KB".to_string(),
        };
    }

    let operation_enum = match operation {
        "Create" => ProposalOperation::Create,
        "Replace" => ProposalOperation::Replace,
        _ => {
            return ToolResult::Error {
                code: "proposal.invalid_input",
                message: "operation must be 'Create' or 'Replace'".to_string(),
            }
        }
    };

    let intent_enum = match intent {
        "Accept" => ProposalIntent::Accept,
        "Rewrite" => ProposalIntent::Rewrite,
        _ => {
            return ToolResult::Error {
                code: "proposal.invalid_input",
                message: "intent must be 'Accept' or 'Rewrite'".to_string(),
            }
        }
    };

    let _profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(_) => {
            return ToolResult::Error {
                code: "store.unreadable",
                message: "Profile could not be loaded".to_string(),
            }
        }
    };

    // Write candidate body to a temp file (submit_proposal requires a file path)
    let temp_dir = store_root.join(".mcp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let candidate_hash = blake3::hash(candidate_body.as_bytes()).to_hex();
    let candidate_path = temp_dir.join(format!("{candidate_hash}.md"));

    if let Err(e) = std::fs::write(&candidate_path, candidate_body) {
        let _ = e;
        return ToolResult::Error {
            code: "proposal.store_error",
            message: "Failed to write candidate file".to_string(),
        };
    }

    // Parse confidence from f64 to Confidence string representation
    // Confidence::parse expects a string like "0.5", not a raw f64
    let parsed_confidence = confidence.and_then(|c| {
        if !(0.0..=1.0).contains(&c) {
            return None;
        }
        // Format as "0.xxx" or "1" for perfect confidence
        let s = if c >= 1.0 {
            "1".to_string()
        } else if c <= 0.0 {
            "0".to_string()
        } else {
            format!("{:.6}", c)
        };
        Confidence::parse(&s).ok()
    });

    // Write evaluator file if present
    let evaluator_path = evaluator.map(|ev| {
        let ev_path = temp_dir.join(format!("{candidate_hash}-eval.json"));
        let _ = std::fs::write(&ev_path, serde_json::to_string(&ev).unwrap());
        ev_path
    });

    let submission = ProposalSubmission {
        profile_path: profile_path.to_path_buf(),
        store_root: store_root.to_path_buf(),
        source_id: source.to_string(),
        operation: operation_enum,
        intent: intent_enum,
        target_id: target_id.to_string(),
        target_path: target_path.to_string(),
        candidate_path,
        confidence: parsed_confidence,
        evaluator_path,
    };

    match submit_proposal(submission) {
        Ok(result) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            ToolResult::Success {
                text: format!(
                    "Proposal submitted. Submission ID: {}",
                    result.submission_id
                ),
                structured: json!({
                    "submission_id": result.submission_id,
                    "state": "submitted",
                    "warnings": [],
                }),
            }
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            ToolResult::Error {
                code: match e {
                    ProposalError::ConfidenceInvalid
                    | ProposalError::IdInvalid
                    | ProposalError::PathUnsafe
                    | ProposalError::MetaParse
                    | ProposalError::MetaNoncanonical
                    | ProposalError::MetaTooLarge
                    | ProposalError::CandidateHashMismatch
                    | ProposalError::CandidateSizeMismatch => "proposal.invalid_input",
                    ProposalError::DuplicateId => "proposal.duplicate_id",
                    ProposalError::Locked | ProposalError::ProtectedLocked => "store.locked",
                    _ => "proposal.store_error",
                },
                message: format!("Proposal failed: {}", e.code()),
            }
        }
    }
}
