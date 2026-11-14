use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use super::{GeneratedDocument, LinkRelation, ProfileConfig, TypedLink};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkConversionResult {
    pub documents: Vec<GeneratedDocument>,
    pub warnings: Vec<LinkWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LinkWarning {
    pub code: String,
    pub source_path: String,
    pub target: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("cross-profile link target in {source_path}: {target}")]
    CrossProfile { source_path: String, target: String },
}

impl LinkError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CrossProfile { .. } => "links.cross_profile",
        }
    }
}

pub fn convert_links(
    profile: &ProfileConfig,
    mut documents: Vec<GeneratedDocument>,
) -> Result<LinkConversionResult, LinkError> {
    documents.sort_by(|left, right| {
        left.metadata
            .source_path
            .cmp(&right.metadata.source_path)
            .then_with(|| left.metadata.id.cmp(&right.metadata.id))
    });
    let index = LinkIndex::new(&documents);
    let mut warnings = Vec::new();

    for document in &mut documents {
        let source_path = document.metadata.source_path.clone();
        let mut links = document.metadata.links.clone();

        for link in &links {
            reject_cross_profile(profile, &source_path, &link.target)?;
            if is_internal_target(&link.target) {
                match index.resolve(&source_path, target_without_fragment(&link.target)) {
                    Resolution::Broken => warnings.push(warning(
                        "links.broken",
                        &source_path,
                        target_without_fragment(&link.target),
                    )),
                    Resolution::Ambiguous => warnings.push(warning(
                        "links.ambiguous",
                        &source_path,
                        target_without_fragment(&link.target),
                    )),
                    Resolution::Found(_) => {}
                }
            }
        }

        collect_markdown_links(
            profile,
            &index,
            &source_path,
            &document.body,
            &mut links,
            &mut warnings,
        )?;
        document.body = convert_wikilinks(
            profile,
            &index,
            &source_path,
            &document.body,
            &mut links,
            &mut warnings,
        )?;

        links.sort();
        links.dedup();
        document.metadata.links = links;
    }

    warnings.sort();
    warnings.dedup();
    Ok(LinkConversionResult {
        documents,
        warnings,
    })
}

fn collect_markdown_links(
    profile: &ProfileConfig,
    index: &LinkIndex,
    source_path: &str,
    body: &str,
    links: &mut Vec<TypedLink>,
    warnings: &mut Vec<LinkWarning>,
) -> Result<(), LinkError> {
    let protected = code_protection_mask(body);
    let mut cursor = 0;
    while let Some(open_offset) = body[cursor..].find('[') {
        let open = cursor + open_offset;
        if protected[open] || is_backslash_escaped(body, open) {
            cursor = open + usize::from(body[open..].starts_with("[[")) + 1;
            continue;
        }
        if body[open..].starts_with("[[") {
            cursor = open + 2;
            continue;
        }
        let Some(close_offset) = body[open + 1..].find(']') else {
            break;
        };
        let close = open + 1 + close_offset;
        if !body[close + 1..].starts_with('(') {
            cursor = close + 1;
            continue;
        }
        let destination_start = close + 2;
        let Some(end) = find_markdown_destination_end(body, destination_start) else {
            break;
        };
        let destination = markdown_destination(&body[destination_start..end]);
        if !destination.is_empty() {
            record_target(profile, index, source_path, destination, links, warnings)?;
        }
        cursor = end + 1;
    }
    Ok(())
}

fn markdown_destination(input: &str) -> &str {
    let trimmed = input.trim();
    if trimmed.starts_with('<') {
        if let Some(end) = find_angle_destination_close(trimmed, 1) {
            return &trimmed[1..end];
        }
    }
    let mut depth = 0;
    let mut escaped = false;
    for (index, character) in trimmed.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            character if character.is_ascii_whitespace() && depth == 0 => {
                return trimmed[..index].trim_end();
            }
            _ => {}
        }
    }
    trimmed
}

fn find_markdown_destination_end(body: &str, start: usize) -> Option<usize> {
    let first_content = body[start..]
        .char_indices()
        .find(|(_, character)| !character.is_ascii_whitespace())
        .map(|(offset, _)| start + offset)?;
    let scan_start = if body[first_content..].starts_with('<') {
        find_angle_destination_close(body, first_content + 1)? + 1
    } else {
        start
    };
    let mut depth = 0;
    let mut title_quote = None;
    let mut escaped = false;
    let mut previous_was_whitespace = false;

    for (offset, character) in body[scan_start..].char_indices() {
        let index = scan_start + offset;
        if escaped {
            escaped = false;
            previous_was_whitespace = character.is_ascii_whitespace();
            continue;
        }
        if character == '\\' {
            escaped = true;
            previous_was_whitespace = false;
            continue;
        }
        if let Some(quote) = title_quote {
            if character == quote {
                title_quote = None;
            }
            previous_was_whitespace = character.is_ascii_whitespace();
            continue;
        }
        if previous_was_whitespace && matches!(character, '\'' | '"') {
            title_quote = Some(character);
            previous_was_whitespace = false;
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(index),
            ')' => depth -= 1,
            _ => {}
        }
        previous_was_whitespace = character.is_ascii_whitespace();
    }
    None
}

fn find_angle_destination_close(body: &str, start: usize) -> Option<usize> {
    let mut escaped = false;
    for (offset, character) in body[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '>' => return Some(start + offset),
            _ => {}
        }
    }
    None
}

fn convert_wikilinks(
    profile: &ProfileConfig,
    index: &LinkIndex,
    source_path: &str,
    body: &str,
    links: &mut Vec<TypedLink>,
    warnings: &mut Vec<LinkWarning>,
) -> Result<String, LinkError> {
    let protected = code_protection_mask(body);
    let mut converted = String::with_capacity(body.len());
    let mut cursor = 0;

    while let Some(open_offset) = body[cursor..].find("[[") {
        let open = cursor + open_offset;
        converted.push_str(&body[cursor..open]);
        if protected[open] || is_backslash_escaped(body, open) {
            converted.push_str("[[");
            cursor = open + 2;
            continue;
        }
        let Some(close_offset) = body[open + 2..].find("]]") else {
            converted.push_str(&body[open..]);
            cursor = body.len();
            break;
        };
        let close = open + 2 + close_offset;
        let original = &body[open..close + 2];
        let contents = &body[open + 2..close];
        let (target_with_fragment, alias) = contents
            .split_once('|')
            .map_or((contents, None), |(target, alias)| (target, Some(alias)));
        let target_with_fragment = target_with_fragment.trim();
        let target = target_without_fragment(target_with_fragment);
        let fragment = target_with_fragment
            .split_once('#')
            .map(|(_, fragment)| fragment);

        reject_cross_profile(profile, source_path, target)?;
        if is_http(target) {
            links.push(TypedLink {
                rel: LinkRelation::Cites,
                target: target_with_fragment.to_owned(),
            });
            let label = alias
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .unwrap_or(target);
            converted.push_str(&format!("[{label}]({target_with_fragment})"));
        } else {
            match index.resolve(source_path, target) {
                Resolution::Found(target_index) => {
                    let target_document = &index.documents[target_index];
                    links.push(TypedLink {
                        rel: LinkRelation::References,
                        target: target_document.id.clone(),
                    });
                    let label = alias
                        .map(str::trim)
                        .filter(|label| !label.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| default_wikilink_label(target, fragment));
                    let mut destination = percent_encode_path(&relative_markdown_path(
                        source_path,
                        &target_document.source_path,
                    ));
                    if let Some(fragment) = fragment {
                        destination.push('#');
                        destination.push_str(&percent_encode_component(fragment));
                    }
                    converted.push_str(&format!("[{label}]({destination})"));
                }
                Resolution::Ambiguous => {
                    warnings.push(warning("links.ambiguous", source_path, target));
                    converted.push_str(original);
                }
                Resolution::Broken => {
                    warnings.push(warning("links.broken", source_path, target));
                    converted.push_str(original);
                }
            }
        }
        cursor = close + 2;
    }

    if cursor < body.len() {
        converted.push_str(&body[cursor..]);
    }
    Ok(converted)
}

fn record_target(
    profile: &ProfileConfig,
    index: &LinkIndex,
    source_path: &str,
    destination: &str,
    links: &mut Vec<TypedLink>,
    warnings: &mut Vec<LinkWarning>,
) -> Result<(), LinkError> {
    reject_cross_profile(profile, source_path, destination)?;
    if is_http(destination) {
        links.push(TypedLink {
            rel: LinkRelation::Cites,
            target: destination.to_owned(),
        });
        return Ok(());
    }
    if has_non_vault_scheme(destination) {
        return Ok(());
    }

    let target = target_without_fragment(destination);
    match index.resolve(source_path, target) {
        Resolution::Found(target_index) => links.push(TypedLink {
            rel: LinkRelation::References,
            target: index.documents[target_index].id.clone(),
        }),
        Resolution::Ambiguous => warnings.push(warning("links.ambiguous", source_path, target)),
        Resolution::Broken => warnings.push(warning("links.broken", source_path, target)),
    }
    Ok(())
}

fn warning(code: &str, source_path: &str, target: &str) -> LinkWarning {
    LinkWarning {
        code: code.to_owned(),
        source_path: source_path.to_owned(),
        target: target.to_owned(),
    }
}

fn reject_cross_profile(
    profile: &ProfileConfig,
    source_path: &str,
    target: &str,
) -> Result<(), LinkError> {
    let target_without_fragment = target_without_fragment(target);
    let namespace = cross_profile_namespace(target_without_fragment);

    if namespace
        .as_deref()
        .is_some_and(|namespace| namespace != profile.context_namespace)
    {
        return Err(LinkError::CrossProfile {
            source_path: source_path.to_owned(),
            target: target.to_owned(),
        });
    }
    Ok(())
}

fn cross_profile_namespace(target: &str) -> Option<String> {
    if let Some(rest) = strip_scheme(target, "vault") {
        rest.split('/').next().map(percent_decode)
    } else if !has_non_vault_scheme(target) {
        percent_decode(target)
            .split('/')
            .find(|segment| looks_like_ssa_namespace(segment))
            .map(str::to_owned)
    } else {
        None
    }
}

fn looks_like_ssa_namespace(segment: &str) -> bool {
    segment.strip_prefix("ssa-").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

fn target_without_fragment(target: &str) -> &str {
    target
        .split_once('#')
        .map_or(target, |(target, _)| target)
        .trim()
}

fn is_http(target: &str) -> bool {
    scheme(target).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

fn has_non_vault_scheme(target: &str) -> bool {
    scheme(target).is_some_and(|scheme| !scheme.eq_ignore_ascii_case("vault"))
        || target
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("mailto:"))
}

fn scheme(target: &str) -> Option<&str> {
    target
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| !scheme.is_empty())
}

fn strip_scheme<'a>(target: &'a str, expected: &str) -> Option<&'a str> {
    let (scheme, rest) = target.split_once("://")?;
    scheme.eq_ignore_ascii_case(expected).then_some(rest)
}

fn is_internal_target(target: &str) -> bool {
    !is_http(target) && !has_non_vault_scheme(target)
}

fn default_wikilink_label(target: &str, fragment: Option<&str>) -> String {
    if target.is_empty() {
        return fragment.unwrap_or_default().to_owned();
    }
    target
        .rsplit('/')
        .next()
        .unwrap_or(target)
        .strip_suffix(".md")
        .unwrap_or_else(|| target.rsplit('/').next().unwrap_or(target))
        .to_owned()
}

fn relative_markdown_path(source_path: &str, target_path: &str) -> String {
    let mut source: Vec<&str> = source_path.split('/').collect();
    source.pop();
    let target: Vec<&str> = target_path.split('/').collect();
    let common = source
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();

    let mut components = vec![".."; source.len() - common];
    components.extend_from_slice(&target[common..]);
    components.join("/")
}

fn percent_encode_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode_component)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_component(component: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(component.len());
    for byte in component.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| input.to_owned())
}

fn code_protection_mask(body: &str) -> Vec<bool> {
    let bytes = body.as_bytes();
    let mut protected = vec![false; bytes.len()];
    let mut fence: Option<(u8, usize)> = None;
    let mut line_start = 0;

    while line_start < bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| line_start + offset + 1);
        let mut content_end = line_end;
        if content_end > line_start && bytes[content_end - 1] == b'\n' {
            content_end -= 1;
        }
        if content_end > line_start && bytes[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        let marker = fence_marker_start(bytes, line_start, content_end).map(|start| {
            let marker_byte = bytes[start];
            let length = bytes[start..content_end]
                .iter()
                .take_while(|byte| **byte == marker_byte)
                .count();
            (start, marker_byte, length)
        });

        if let Some((fence_byte, fence_length)) = fence {
            protected[line_start..line_end].fill(true);
            if let Some((start, marker_byte, marker_length)) = marker {
                if marker_byte == fence_byte
                    && marker_length >= fence_length
                    && bytes[start + marker_length..content_end]
                        .iter()
                        .all(|byte| byte.is_ascii_whitespace())
                {
                    fence = None;
                }
            }
        } else if let Some((_, marker_byte, marker_length)) = marker {
            if marker_length >= 3 {
                protected[line_start..line_end].fill(true);
                fence = Some((marker_byte, marker_length));
            }
        }
        line_start = line_end;
    }
    let mut region_start = 0;
    while region_start < bytes.len() {
        while region_start < bytes.len() && protected[region_start] {
            region_start += 1;
        }
        let mut region_end = region_start;
        while region_end < bytes.len() && !protected[region_end] {
            region_end += 1;
        }
        protect_inline_code(body, region_start, region_end, &mut protected);
        region_start = region_end;
    }
    protected
}

fn fence_marker_start(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut marker = start;
    while marker < end && marker - start < 3 && bytes[marker] == b' ' {
        marker += 1;
    }
    (marker < end && matches!(bytes[marker], b'`' | b'~')).then_some(marker)
}

fn protect_inline_code(body: &str, start: usize, end: usize, protected: &mut [bool]) {
    let bytes = body.as_bytes();
    let mut cursor = start;
    while cursor < end {
        if protected[cursor] || bytes[cursor] != b'`' || is_backslash_escaped(body, cursor) {
            cursor += 1;
            continue;
        }
        let delimiter_length = bytes[cursor..end]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let mut candidate = cursor + delimiter_length;
        let mut closing_end = None;
        while candidate < end {
            if protected[candidate] || bytes[candidate] != b'`' {
                candidate += 1;
                continue;
            }
            let run_length = bytes[candidate..end]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if run_length == delimiter_length {
                closing_end = Some(candidate + run_length);
                break;
            }
            candidate += run_length;
        }
        if let Some(closing_end) = closing_end {
            protected[cursor..closing_end].fill(true);
            cursor = closing_end;
        } else {
            cursor += delimiter_length;
        }
    }
}

fn is_backslash_escaped(body: &str, index: usize) -> bool {
    body.as_bytes()[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug)]
struct IndexedDocument {
    source_path: String,
    id: String,
    resource: String,
}

struct LinkIndex {
    documents: Vec<IndexedDocument>,
    exact: BTreeMap<String, BTreeSet<usize>>,
    basenames: BTreeMap<String, BTreeSet<usize>>,
}

impl LinkIndex {
    fn new(documents: &[GeneratedDocument]) -> Self {
        let indexed: Vec<_> = documents
            .iter()
            .map(|document| IndexedDocument {
                source_path: document.metadata.source_path.clone(),
                id: document.metadata.id.clone(),
                resource: document.metadata.resource.clone(),
            })
            .collect();
        let mut exact = BTreeMap::<String, BTreeSet<usize>>::new();
        let mut basenames = BTreeMap::<String, BTreeSet<usize>>::new();

        for (index, document) in indexed.iter().enumerate() {
            for key in [
                document.source_path.clone(),
                strip_markdown_extension(&document.source_path).to_owned(),
                document.id.clone(),
                document.resource.clone(),
            ] {
                exact.entry(key).or_default().insert(index);
            }
            let basename = document
                .source_path
                .rsplit('/')
                .next()
                .map(strip_markdown_extension)
                .unwrap_or_default();
            basenames
                .entry(basename.to_owned())
                .or_default()
                .insert(index);
        }

        Self {
            documents: indexed,
            exact,
            basenames,
        }
    }

    fn resolve(&self, source_path: &str, target: &str) -> Resolution {
        if target.is_empty() {
            return self.exact_resolution(source_path);
        }
        if let Some(rest) = strip_scheme(target, "vault") {
            let direct = self.exact_resolution(&format!("vault://{rest}"));
            if direct != Resolution::Broken {
                return direct;
            }
            return self.exact_resolution(&decode_vault_namespace(rest));
        }
        if target.starts_with("ssa-") {
            return self.exact_resolution(target);
        }

        let decoded_target = percent_decode(target);
        let target = decoded_target.as_str();

        if let Some(relative) = normalize_relative_path(source_path, target) {
            let resolution = self.exact_resolution(&relative);
            if resolution != Resolution::Broken {
                return resolution;
            }
        }
        let root_relative = target.trim_start_matches('/');
        let resolution = self.exact_resolution(root_relative);
        if resolution != Resolution::Broken {
            return resolution;
        }

        let basename = target
            .rsplit('/')
            .next()
            .map(strip_markdown_extension)
            .unwrap_or_default();
        resolution_for(self.basenames.get(basename))
    }

    fn exact_resolution(&self, target: &str) -> Resolution {
        let direct = resolution_for(self.exact.get(target));
        if direct != Resolution::Broken {
            return direct;
        }
        if target.ends_with(".md") {
            resolution_for(self.exact.get(strip_markdown_extension(target)))
        } else {
            resolution_for(self.exact.get(&format!("{target}.md")))
        }
    }
}

fn decode_vault_namespace(rest: &str) -> String {
    let (namespace, path) = rest
        .split_once('/')
        .map_or((rest, None), |(namespace, path)| (namespace, Some(path)));
    let mut target = format!("vault://{}", percent_decode(namespace));
    if let Some(path) = path {
        target.push('/');
        target.push_str(path);
    }
    target
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    Found(usize),
    Ambiguous,
    Broken,
}

fn resolution_for(indices: Option<&BTreeSet<usize>>) -> Resolution {
    match indices {
        Some(indices) if indices.len() == 1 => Resolution::Found(*indices.first().unwrap()),
        Some(indices) if indices.len() > 1 => Resolution::Ambiguous,
        _ => Resolution::Broken,
    }
}

fn normalize_relative_path(source_path: &str, target: &str) -> Option<String> {
    let mut components: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        let mut source: Vec<&str> = source_path.split('/').collect();
        source.pop();
        source
    };
    for component in target.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            component => components.push(component),
        }
    }
    Some(components.join("/"))
}

fn strip_markdown_extension(path: &str) -> &str {
    path.strip_suffix(".md").unwrap_or(path)
}
