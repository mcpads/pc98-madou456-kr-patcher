use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};

pub(crate) const PARTICLE_MARKERS: [char; 4] = ['\u{E000}', '\u{E001}', '\u{E002}', '\u{E003}'];
pub(crate) const PARTICLE_FORMS: [char; 8] = ['를', '을', '가', '이', '는', '은', '와', '과'];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum KoreanParticle {
    Object,
    Subject,
    Topic,
    With,
}

impl KoreanParticle {
    pub(crate) const ALL: [Self; 4] = [Self::Object, Self::Subject, Self::Topic, Self::With];

    pub(crate) fn parse(form: &str) -> Option<Self> {
        match form {
            "을" | "를" => Some(Self::Object),
            "이" | "가" => Some(Self::Subject),
            "은" | "는" => Some(Self::Topic),
            "와" | "과" => Some(Self::With),
            _ => None,
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Object => 0,
            Self::Subject => 1,
            Self::Topic => 2,
            Self::With => 3,
        }
    }

    pub(crate) const fn marker(self) -> char {
        PARTICLE_MARKERS[self.index()]
    }

    pub(crate) const fn forms(self) -> (char, char) {
        match self {
            Self::Object => ('를', '을'),
            Self::Subject => ('가', '이'),
            Self::Topic => ('는', '은'),
            Self::With => ('와', '과'),
        }
    }

    #[cfg(test)]
    pub(crate) fn select_for(self, preceding: char) -> Result<char> {
        let (without_batchim, with_batchim) = self.forms();
        Ok(if has_batchim(preceding)? {
            with_batchim
        } else {
            without_batchim
        })
    }
}

pub(crate) fn runtime_text_characters(text: &str) -> Result<Vec<char>> {
    let mut characters = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let remaining = &text[cursor..];
        if let Some(marker) = remaining.strip_prefix("{josa:") {
            let close = marker
                .find('}')
                .context("unterminated runtime particle marker")?;
            let form = &marker[..close];
            let particle = KoreanParticle::parse(form)
                .with_context(|| format!("unsupported runtime particle form {form:?}"))?;
            characters.push(particle.marker());
            cursor += "{josa:".len() + close + 1;
            continue;
        }

        let character = remaining
            .chars()
            .next()
            .expect("nonempty UTF-8 suffix has one character");
        if is_particle_marker(character) {
            bail!(
                "translation text contains raw runtime particle marker U+{:04X}",
                character as u32
            );
        }
        characters.push(character);
        cursor += character.len_utf8();
    }
    Ok(characters)
}

pub(crate) fn manual_message_lines(text: &str) -> Result<Vec<&str>> {
    if text.is_empty() {
        return Ok(vec![text]);
    }
    let lines = text.split('\n').collect::<Vec<_>>();
    ensure!(
        lines.iter().all(|line| !line.is_empty()),
        "manual message line break must have text on both sides"
    );
    for line in &lines {
        runtime_text_characters(line)?;
    }
    Ok(lines)
}

pub(crate) fn plan_translation_characters(characters: &BTreeSet<char>) -> Result<Vec<char>> {
    let symbols = characters
        .iter()
        .copied()
        .filter(|character| !is_modern_hangul(*character) && !is_particle_marker(*character))
        .collect::<Vec<_>>();
    let mut hangul = characters
        .iter()
        .copied()
        .filter(|character| is_modern_hangul(*character))
        .collect::<BTreeSet<_>>();
    hangul.extend(PARTICLE_FORMS);
    let (without_batchim, with_batchim): (Vec<_>, Vec<_>) = hangul
        .into_iter()
        .partition(|character| !has_batchim(*character).expect("drawable entries are Hangul"));
    Ok(symbols
        .into_iter()
        .chain(without_batchim)
        .chain(with_batchim)
        .chain(PARTICLE_MARKERS)
        .collect())
}

pub(crate) fn has_batchim(character: char) -> Result<bool> {
    if !is_modern_hangul(character) {
        bail!("particle selection requires a precomposed Hangul syllable, got {character:?}");
    }
    Ok(!(character as u32 - '가' as u32).is_multiple_of(28))
}

pub(crate) fn is_modern_hangul(character: char) -> bool {
    ('가'..='힣').contains(&character)
}

pub(crate) fn is_particle_marker(character: char) -> bool {
    PARTICLE_MARKERS.contains(&character)
}

#[cfg(test)]
#[path = "josa_tests.rs"]
mod tests;
