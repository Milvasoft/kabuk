//! The model's reply: its ```toml and ```hlsl blocks.

/// The two files a reply carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    pub toml: String,
    pub hlsl: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    #[error("the reply has no ```toml block for wall.toml")]
    NoToml,
    #[error("the reply has no ```hlsl block for wall.hlsl")]
    NoHlsl,
}

/// The fenced blocks of `text`, each with its language (lowercase, may be empty).
fn blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in text.lines() {
        let fence = line.trim_start().strip_prefix("```");
        if let Some(lang) = fence {
            match open.take() {
                Some(done) => out.push(done),
                None => open = Some((lang.trim().to_ascii_lowercase(), String::new())),
            }
        } else if let Some((_, body)) = open.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    // A reply cut off inside its last block keeps what it has.
    out.extend(open);
    out
}

/// The first ```toml block and the first ```hlsl one (a block marked `c`, `cpp` or
/// `glsl`, or with no language, stands in for HLSL when there is none).
pub fn files(text: &str) -> Result<Files, ReplyError> {
    let all = blocks(text);
    let toml = all
        .iter()
        .find(|(l, _)| l == "toml")
        .map(|(_, b)| b.clone())
        .ok_or(ReplyError::NoToml)?;
    let hlsl = all
        .iter()
        .find(|(l, _)| l == "hlsl")
        .or_else(|| {
            all.iter()
                .find(|(l, _)| matches!(l.as_str(), "c" | "cpp" | "glsl" | ""))
        })
        .map(|(_, b)| b.clone())
        .ok_or(ReplyError::NoHlsl)?;
    Ok(Files { toml, hlsl })
}

#[cfg(test)]
mod tests {
    //! Edge cases: both blocks among prose; indented fences; a missing block; HLSL marked
    //! as another C-like language; a reply cut off inside its last block.
    use super::*;

    #[test]
    fn both_files_come_out_of_the_reply() {
        let r = "Here you go:\n\n```toml\nname = \"x\"\n```\n\nAnd the shader:\n  ```HLSL\nfloat3 wall(float2 px) { return 0; }\n  ```\nEnjoy.";
        let f = files(r).unwrap();
        assert_eq!(f.toml, "name = \"x\"\n");
        assert_eq!(f.hlsl, "float3 wall(float2 px) { return 0; }\n");
    }

    #[test]
    fn a_missing_block_is_named() {
        assert_eq!(files("```hlsl\nx\n```"), Err(ReplyError::NoToml));
        assert_eq!(files("```toml\nname = \"x\"\n```"), Err(ReplyError::NoHlsl));
        assert_eq!(files("no code at all"), Err(ReplyError::NoToml));
    }

    #[test]
    fn other_c_like_blocks_and_cut_replies() {
        let f = files("```toml\nname = \"x\"\n```\n```cpp\nfloat3 wall(float2 px) {}\n```").unwrap();
        assert!(f.hlsl.contains("wall"));
        let cut = files("```toml\nname = \"x\"\n```\n```hlsl\nfloat3 wall(float2 px)\n{").unwrap();
        assert_eq!(cut.hlsl, "float3 wall(float2 px)\n{\n");
    }
}
