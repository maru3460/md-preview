//! 送り手が名乗った情報（#68）。パイプ入力と `--notify` に `--title` / `--description` /
//! `--note` で添えられる。出る場所はフラグごとに決めてあり、description と note が
//! 同じ画面に並ぶことは無い（title だけは名前として使い回す）。
//!
//! - `title`：タブ名（パイプ入力のときだけ）とベルの行の名前。コメントのラベルにも使う
//! - `description`：ベルの行の 2 段目
//! - `note`：タブの下の帯（パイプ入力のタブと、ベルから開いたタブ）
//!
//! **md は渡された値を映すだけ。** 何も渡されなければ何も出さないし、環境変数や
//! エージェントの内部ファイルから補うこともしない（新しい環境が出るたびに md を
//! 直すことになるため。何を名乗るかは skill がエージェントに指示する）。
//!
//! 値はどれもテキストとして扱う。ここで制御文字を潰しておくので、この先の書式
//! （転送の NUL 区切り、台帳の改行区切り）は値の中身を気にしなくてよい。

use crate::html::json_string;

/// 1 つの値の長さの上限（文字数）。見た目の省略は CSS が持つので、これは表示の
/// ためではなく、ベルの台帳（最大 100 行）に毎行載るものを人が読める長さに留めるため。
/// 転送の 1 通の上限（1 MiB）とは関係が無い。
///
/// Why not skill に書いた字数の目安で切る: 英語と日本語で同じ字数の重さが
/// まるで違う。見た目の長さは CSS の省略記号に任せ、ここは壊さないための上限だけ持つ。
const MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    /// `--title`。パイプ入力のタブ名になる（`stdin.md` の代わり）。
    pub title: Option<String>,
    /// `--description`。どの作業の何か。ベルにだけ出る。
    pub description: Option<String>,
    /// `--note`。開いた人が読む補足。帯に出る。
    pub note: Option<String>,
}

impl Origin {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none() && self.note.is_none()
    }

    /// ページへ渡す形。`{title, description, note}`（無いものは `null`）。
    pub fn to_json(&self) -> String {
        let opt = |v: &Option<String>| v.as_deref().map(json_string).unwrap_or_else(|| "null".into());
        format!(
            "{{\"title\":{},\"description\":{},\"note\":{}}}",
            opt(&self.title),
            opt(&self.description),
            opt(&self.note)
        )
    }
}

/// 名乗りの値を表示できる形に整える。制御文字（改行・タブ・NUL を含む）は空白に
/// 寄せ、前後の空白を落とし、長すぎれば切る。空になったら `None`。
pub fn clean(s: &str) -> Option<String> {
    let s: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.chars().count() > MAX_CHARS {
        let cut: String = s.chars().take(MAX_CHARS - 1).collect();
        return Some(format!("{}…", cut.trim_end()));
    }
    Some(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_become_spaces_and_the_ends_are_trimmed() {
        assert_eq!(clean("  a\nb\tc\0d  ").as_deref(), Some("a b c d"));
    }

    #[test]
    fn a_blank_value_is_no_value() {
        assert_eq!(clean(" \n "), None);
    }

    #[test]
    fn a_long_value_is_cut_with_an_ellipsis() {
        let got = clean(&"あ".repeat(MAX_CHARS + 50)).unwrap();
        assert_eq!(got.chars().count(), MAX_CHARS);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn json_escapes_the_values_and_writes_null_for_what_is_missing() {
        let o = Origin { title: Some("a\"b".into()), note: Some("n".into()), ..Default::default() };
        assert_eq!(o.to_json(), r#"{"title":"a\"b","description":null,"note":"n"}"#);
    }
}
