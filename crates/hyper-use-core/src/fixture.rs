use crate::error::{CoreError, FixtureError};
use crate::id::{RegionId, UnitInterval};
use crate::manifold::InteractionManifold;
use crate::rect::Rect;
use crate::region::{InteractionRegion, RegionParts};
use crate::vocab::{Action, RegionFlags, Role, SourceMask};

/// Parse a static manifold fixture.
///
/// ```text
/// viewport w=1440 h=900
/// region id=nav-settings role=button label="Settings" x=16 y=180 w=200 h=36 actions=click parent=nav sources=dom,accessibility
/// ```
///
/// Lines starting with `#` and blank lines are ignored. `viewport` is required
/// once. Region keys: `id`, `role`, `label`, `x`, `y`, `w`, `h`, and optional
/// `actions`, `parent`, `sources`, `flags`, `stability`.
pub fn parse_fixture(input: &str) -> Result<InteractionManifold, FixtureError> {
    let mut viewport: Option<Rect> = None;
    let mut regions = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let tokens = split_tokens(line).map_err(|message| FixtureError::Line {
            line: line_no,
            message,
        })?;
        if tokens.is_empty() {
            continue;
        }
        match tokens[0].as_str() {
            "viewport" => {
                if viewport.is_some() {
                    return Err(FixtureError::DuplicateViewport { line: line_no });
                }
                viewport = Some(parse_viewport(&tokens[1..]).map_err(|message| {
                    FixtureError::Line {
                        line: line_no,
                        message,
                    }
                })?);
            }
            "region" => {
                let region = parse_region(&tokens[1..]).map_err(|message| FixtureError::Line {
                    line: line_no,
                    message,
                })?;
                if regions.iter().any(|existing: &InteractionRegion| {
                    existing.id() == region.id()
                }) {
                    return Err(FixtureError::DuplicateRegion {
                        line: line_no,
                        id: region.id().to_string(),
                    });
                }
                regions.push(region);
            }
            other => {
                return Err(FixtureError::Line {
                    line: line_no,
                    message: format!("unknown directive `{other}`"),
                });
            }
        }
    }
    let viewport = viewport.ok_or(FixtureError::MissingViewport)?;
    InteractionManifold::try_new(viewport, regions, 0).map_err(|err| match err {
        CoreError::DuplicateRegion(id) => FixtureError::DuplicateRegion { line: 0, id },
        other => FixtureError::Line {
            line: 0,
            message: other.to_string(),
        },
    })
}

fn strip_comment(line: &str) -> &str {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn split_tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' if in_quotes => {
                let Some(escaped) = chars.next() else {
                    return Err("trailing escape in quoted string".to_owned());
                };
                match escaped {
                    'n' => buf.push('\n'),
                    't' => buf.push('\t'),
                    other => buf.push(other),
                }
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            ch => buf.push(ch),
        }
    }
    if in_quotes {
        return Err("unclosed quote".to_owned());
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    Ok(out)
}

fn parse_viewport(tokens: &[String]) -> Result<Rect, String> {
    let map = pairs(tokens)?;
    let width = required_f64(&map, "w")?;
    let height = required_f64(&map, "h")?;
    Rect::try_viewport(0.0, 0.0, width, height).map_err(|err| err.to_string())
}

fn parse_region(tokens: &[String]) -> Result<InteractionRegion, String> {
    let map = pairs(tokens)?;
    let id = RegionId::try_new(required(&map, "id")?).map_err(|err| err.to_string())?;
    let role = Role::parse(required(&map, "role")?)
        .ok_or_else(|| format!("unknown role `{}`", required(&map, "role").unwrap_or("")))?;
    let label = required(&map, "label")?.to_owned();
    let rect = Rect::try_new(
        required_f64(&map, "x")?,
        required_f64(&map, "y")?,
        required_f64(&map, "w")?,
        required_f64(&map, "h")?,
    )
    .map_err(|err| err.to_string())?;
    let actions = match optional(&map, "actions") {
        None => Vec::new(),
        Some("") => Vec::new(),
        Some(raw) => {
            let mut actions = Vec::new();
            for part in raw.split(',') {
                let action = Action::parse(part.trim())
                    .ok_or_else(|| format!("unknown action `{}`", part.trim()))?;
                actions.push(action);
            }
            actions
        }
    };
    let parent = match optional(&map, "parent") {
        None | Some("") => None,
        Some(raw) => Some(RegionId::try_new(raw).map_err(|err| err.to_string())?),
    };
    let sources = match optional(&map, "sources") {
        None => SourceMask::NONE,
        Some(raw) => SourceMask::parse_list(raw)?,
    };
    let flags = match optional(&map, "flags") {
        None => RegionFlags::none(),
        Some(raw) => RegionFlags::parse_list(raw)?,
    };
    let temporal_stability = match optional(&map, "stability") {
        None => UnitInterval::ONE,
        Some(raw) => {
            let value: f64 = raw
                .parse()
                .map_err(|_| format!("stability `{raw}` is not a number"))?;
            UnitInterval::try_new(value).map_err(|err| err.to_string())?
        }
    };
    InteractionRegion::try_new(RegionParts {
        id,
        role,
        label,
        rect,
        actions,
        parent,
        sources,
        flags,
        temporal_stability,
    })
    .map_err(|err| err.to_string())
}

fn pairs(tokens: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut map = Vec::new();
    for token in tokens {
        let Some((key, value)) = token.split_once('=') else {
            return Err(format!("expected key=value, found `{token}`"));
        };
        if key.is_empty() {
            return Err(format!("empty key in `{token}`"));
        }
        if map.iter().any(|(existing, _)| existing == key) {
            return Err(format!("duplicate key `{key}`"));
        }
        map.push((key.to_owned(), value.to_owned()));
    }
    Ok(map)
}

fn optional<'a>(map: &'a [(String, String)], key: &str) -> Option<&'a str> {
    map.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn required<'a>(map: &'a [(String, String)], key: &str) -> Result<&'a str, String> {
    optional(map, key).ok_or_else(|| format!("missing `{key}`"))
}

fn required_f64(map: &[(String, String)], key: &str) -> Result<f64, String> {
    let raw = required(map, key)?;
    raw.parse::<f64>()
        .map_err(|_| format!("`{key}` value `{raw}` is not a number"))
        .and_then(|value| {
            if value.is_finite() {
                Ok(value)
            } else {
                Err(format!("`{key}` must be finite"))
            }
        })
}
