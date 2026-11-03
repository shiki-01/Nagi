//! 読み（ひらがな）→ ローマ字のキー経路。
//!
//! 見出し語は読みなので、なぞり軌跡と比べるには「その読みをローマ字で打つときに通るキーの列」が要る。
//! 綴りの揺れは代表の綴りに正規化し、残りの揺れは位置のばらつき（σ）で吸収する:
//!
//! - 代表の綴り（`Style::Canon`）: し = `si`、ち = `ti`、つ = `tu`、ふ = `hu`、じ = `zi`、しゃ = `sya` など
//!   （最短の綴り）。ヘボン式（`Style::Hepburn`: `shi` `chi` `tsu` `fu` `ji` `sha` `cha` `ja`）も
//!   第 2 の綴りとして持ち、先頭・末尾のキーが違うものは索引で両方の箱に入れる
//! - 同じキーが続く並び（促音の子音重ね `kk`、撥音の `nn`、母音の連続 `oo`）は 1 つのキーにまとめ、
//!   「小さなループ」付きの印を付ける。軌跡は同じキーの上で止まるか小さく回るだけなので、
//!   ループなし・ありの 2 通りの理想経路を照合して良い方を採る
//! - 長音 `ー` はキーが無いので飛ばす（`こーひー` → `kohi`）
//! - ひらがな以外（漢字・記号・数字・カタカナ）を含む読みは経路にできない（`None`）

/// 綴りの流儀
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// 代表の綴り（最短）
    Canon,
    /// ヘボン式（`shi` `chi` `tsu` `fu` `ji`）
    Hepburn,
}

/// キー経路: 文字の番号（`a` = 0）の列と、各キーに「同じキーが続いた」印
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyPath {
    /// 通るキー（連続する同じキーは 1 つにまとめてある）
    pub keys: Vec<u8>,
    /// `keys[i]` が同じキーの連続（小さなループ）を含むか
    pub loops: Vec<bool>,
}

impl KeyPath {
    /// 先頭のキー
    pub fn first(&self) -> Option<u8> {
        self.keys.first().copied()
    }

    /// 末尾のキー
    pub fn last(&self) -> Option<u8> {
        self.keys.last().copied()
    }

    /// ループを含むキーがあるか
    pub fn has_loops(&self) -> bool {
        self.loops.iter().any(|l| *l)
    }
}

/// 1 文字の仮名の綴り（代表, ヘボン）
fn spelling1(c: char) -> Option<(&'static str, &'static str)> {
    let same = |x: &'static str| Some((x, x));
    match c {
        // 母音
        'あ' => same("a"),
        'い' => same("i"),
        'う' => same("u"),
        'え' => same("e"),
        'お' => same("o"),
        'ぁ' => same("xa"),
        'ぃ' => same("xi"),
        'ぅ' => same("xu"),
        'ぇ' => same("xe"),
        'ぉ' => same("xo"),
        'ゃ' => same("xya"),
        'ゅ' => same("xyu"),
        'ょ' => same("xyo"),
        'ゎ' => same("xwa"),
        // か行・が行
        'か' => same("ka"),
        'き' => same("ki"),
        'く' => same("ku"),
        'け' => same("ke"),
        'こ' => same("ko"),
        'が' => same("ga"),
        'ぎ' => same("gi"),
        'ぐ' => same("gu"),
        'げ' => same("ge"),
        'ご' => same("go"),
        // さ行・ざ行
        'さ' => same("sa"),
        'し' => Some(("si", "shi")),
        'す' => same("su"),
        'せ' => same("se"),
        'そ' => same("so"),
        'ざ' => same("za"),
        'じ' => Some(("zi", "ji")),
        'ず' => same("zu"),
        'ぜ' => same("ze"),
        'ぞ' => same("zo"),
        // た行・だ行
        'た' => same("ta"),
        'ち' => Some(("ti", "chi")),
        'つ' => Some(("tu", "tsu")),
        'て' => same("te"),
        'と' => same("to"),
        'だ' => same("da"),
        'ぢ' => same("di"),
        'づ' => same("du"),
        'で' => same("de"),
        'ど' => same("do"),
        // な行
        'な' => same("na"),
        'に' => same("ni"),
        'ぬ' => same("nu"),
        'ね' => same("ne"),
        'の' => same("no"),
        // は行・ば行・ぱ行
        'は' => same("ha"),
        'ひ' => same("hi"),
        'ふ' => Some(("hu", "fu")),
        'へ' => same("he"),
        'ほ' => same("ho"),
        'ば' => same("ba"),
        'び' => same("bi"),
        'ぶ' => same("bu"),
        'べ' => same("be"),
        'ぼ' => same("bo"),
        'ぱ' => same("pa"),
        'ぴ' => same("pi"),
        'ぷ' => same("pu"),
        'ぺ' => same("pe"),
        'ぽ' => same("po"),
        // ま行・や行・ら行・わ行
        'ま' => same("ma"),
        'み' => same("mi"),
        'む' => same("mu"),
        'め' => same("me"),
        'も' => same("mo"),
        'や' => same("ya"),
        'ゆ' => same("yu"),
        'よ' => same("yo"),
        'ら' => same("ra"),
        'り' => same("ri"),
        'る' => same("ru"),
        'れ' => same("re"),
        'ろ' => same("ro"),
        'わ' => same("wa"),
        'ゐ' => same("wi"),
        'ゑ' => same("we"),
        'を' => same("wo"),
        'ゔ' => same("vu"),
        _ => None,
    }
}

/// 2 文字の仮名（拗音・外来音）の綴り（代表, ヘボン）
fn spelling2(a: char, b: char) -> Option<(&'static str, &'static str)> {
    let same = |x: &'static str| Some((x, x));
    match (a, b) {
        // 拗音
        ('き', 'ゃ') => same("kya"),
        ('き', 'ゅ') => same("kyu"),
        ('き', 'ょ') => same("kyo"),
        ('ぎ', 'ゃ') => same("gya"),
        ('ぎ', 'ゅ') => same("gyu"),
        ('ぎ', 'ょ') => same("gyo"),
        ('し', 'ゃ') => Some(("sya", "sha")),
        ('し', 'ゅ') => Some(("syu", "shu")),
        ('し', 'ょ') => Some(("syo", "sho")),
        ('し', 'ぇ') => Some(("sye", "she")),
        ('じ', 'ゃ') => Some(("zya", "ja")),
        ('じ', 'ゅ') => Some(("zyu", "ju")),
        ('じ', 'ょ') => Some(("zyo", "jo")),
        ('じ', 'ぇ') => Some(("zye", "je")),
        ('ち', 'ゃ') => Some(("tya", "cha")),
        ('ち', 'ゅ') => Some(("tyu", "chu")),
        ('ち', 'ょ') => Some(("tyo", "cho")),
        ('ち', 'ぇ') => Some(("tye", "che")),
        ('ぢ', 'ゃ') => same("dya"),
        ('ぢ', 'ゅ') => same("dyu"),
        ('ぢ', 'ょ') => same("dyo"),
        ('に', 'ゃ') => same("nya"),
        ('に', 'ゅ') => same("nyu"),
        ('に', 'ょ') => same("nyo"),
        ('ひ', 'ゃ') => same("hya"),
        ('ひ', 'ゅ') => same("hyu"),
        ('ひ', 'ょ') => same("hyo"),
        ('び', 'ゃ') => same("bya"),
        ('び', 'ゅ') => same("byu"),
        ('び', 'ょ') => same("byo"),
        ('ぴ', 'ゃ') => same("pya"),
        ('ぴ', 'ゅ') => same("pyu"),
        ('ぴ', 'ょ') => same("pyo"),
        ('み', 'ゃ') => same("mya"),
        ('み', 'ゅ') => same("myu"),
        ('み', 'ょ') => same("myo"),
        ('り', 'ゃ') => same("rya"),
        ('り', 'ゅ') => same("ryu"),
        ('り', 'ょ') => same("ryo"),
        // 外来音
        ('ふ', 'ぁ') => same("fa"),
        ('ふ', 'ぃ') => same("fi"),
        ('ふ', 'ぇ') => same("fe"),
        ('ふ', 'ぉ') => same("fo"),
        ('て', 'ぃ') => same("thi"),
        ('て', 'ゅ') => same("thu"),
        ('で', 'ぃ') => same("dhi"),
        ('で', 'ゅ') => same("dhu"),
        ('と', 'ぅ') => same("twu"),
        ('ど', 'ぅ') => same("dwu"),
        ('う', 'ぃ') => same("wi"),
        ('う', 'ぇ') => same("we"),
        ('う', 'ぉ') => same("who"),
        ('ゔ', 'ぁ') => same("va"),
        ('ゔ', 'ぃ') => same("vi"),
        ('ゔ', 'ぇ') => same("ve"),
        ('ゔ', 'ぉ') => same("vo"),
        ('く', 'ぁ') => same("kwa"),
        ('く', 'ぃ') => same("kwi"),
        ('く', 'ぇ') => same("kwe"),
        ('く', 'ぉ') => same("kwo"),
        ('ぐ', 'ぁ') => same("gwa"),
        ('つ', 'ぁ') => same("tsa"),
        ('つ', 'ぃ') => same("tsi"),
        ('つ', 'ぇ') => same("tse"),
        ('つ', 'ぉ') => same("tso"),
        _ => None,
    }
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'i' | b'u' | b'e' | b'o')
}

/// `chars[i..i + len]`（1 または 2 文字）を 1 つの仮名として引く
fn token(chars: &[char], i: usize, len: usize) -> Option<(&'static str, &'static str)> {
    match len {
        1 => spelling1(*chars.get(i)?),
        _ => spelling2(*chars.get(i)?, *chars.get(i + 1)?),
    }
}

/// 読みを綴り（小文字の英字列）にして `out` に作り直す。`chars` は作業用。
/// ひらがなと `ー` 以外があれば `false`
fn romanize(reading: &str, style: Style, out: &mut Vec<u8>, chars: &mut Vec<char>) -> bool {
    out.clear();
    chars.clear();
    chars.extend(reading.chars());
    let chars = &chars[..];
    let mut pending_sokuon = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == 'ー' {
            i += 1;
            continue;
        }
        if c == 'っ' {
            // 次のトークンの子音を重ねる。次が無い・母音なら `xtu`
            pending_sokuon = true;
            i += 1;
            continue;
        }
        if c == 'ん' {
            // 次の綴りが母音・y・n で始まるなら `nn`、そうでなければ `n`
            let nn = next_initial(chars, i + 1, style)
                .is_some_and(|b| is_vowel(b) || b == b'y' || b == b'n');
            if pending_sokuon {
                // 「っん」は不正な並び。`xtu` を出してから ん を続ける
                out.extend_from_slice(b"xtu");
                pending_sokuon = false;
            }
            out.push(b'n');
            if nn {
                out.push(b'n');
            }
            i += 1;
            continue;
        }
        // 2 文字の仮名を優先
        let (spell, adv) = if let Some(sp) = token(chars, i, 2) {
            (sp, 2)
        } else if let Some(sp) = token(chars, i, 1) {
            (sp, 1)
        } else {
            return false;
        };
        let text = match style {
            Style::Canon => spell.0,
            Style::Hepburn => spell.1,
        }
        .as_bytes();
        if pending_sokuon {
            if is_vowel(text[0]) {
                out.extend_from_slice(b"xtu");
            } else {
                out.push(text[0]);
            }
            pending_sokuon = false;
        }
        out.extend_from_slice(text);
        i += adv;
    }
    if pending_sokuon {
        out.extend_from_slice(b"xtu");
    }
    !out.is_empty()
}

/// `chars[from..]` の次のトークンの綴りの先頭文字（`ー` と `っ` は飛ばす。`っ` は子音を重ねるので次の先頭と同じ）
fn next_initial(chars: &[char], from: usize, style: Style) -> Option<u8> {
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            'ー' | 'っ' => i += 1,
            'ん' => return Some(b'n'),
            _ => {
                return token(chars, i, 2)
                    .or_else(|| token(chars, i, 1))
                    .map(|sp| initial(sp, style));
            }
        }
    }
    None
}

fn initial(sp: (&'static str, &'static str), style: Style) -> u8 {
    match style {
        Style::Canon => sp.0.as_bytes()[0],
        Style::Hepburn => sp.1.as_bytes()[0],
    }
}

/// 綴りから、連続する同じキーを 1 つにまとめたキー経路を `path` に作り直す
fn collapse(letters: &[u8], path: &mut KeyPath) {
    path.keys.clear();
    path.loops.clear();
    for &b in letters {
        if !b.is_ascii_lowercase() {
            continue;
        }
        let k = b - b'a';
        if path.keys.last() == Some(&k) {
            *path.loops.last_mut().expect("非空") = true;
        } else {
            path.keys.push(k);
            path.loops.push(false);
        }
    }
}

/// 経路を繰り返し作るときの作業領域（確保を繰り返さないため）
#[derive(Debug, Default)]
pub struct Speller {
    letters: Vec<u8>,
    chars: Vec<char>,
}

impl Speller {
    /// 読みのキー経路を `path` に作る。経路にできない（ひらがな以外を含む）か、
    /// キーが 2 つ未満（タップと区別できない）なら `false`
    pub fn path(&mut self, reading: &str, style: Style, path: &mut KeyPath) -> bool {
        if !romanize(reading, style, &mut self.letters, &mut self.chars) {
            return false;
        }
        collapse(&self.letters, path);
        path.keys.len() >= 2
    }
}

/// 読みのキー経路。経路にできない（ひらがな以外を含む）か、キーが 2 つ未満（タップと区別できない）なら `None`
pub fn key_path(reading: &str, style: Style) -> Option<KeyPath> {
    let mut path = KeyPath::default();
    Speller::default()
        .path(reading, style, &mut path)
        .then_some(path)
}

/// 読みの綴り（確認・テスト用）。経路にできなければ `None`
pub fn spelling_of(reading: &str, style: Style) -> Option<String> {
    let (mut letters, mut chars) = (Vec::new(), Vec::new());
    romanize(reading, style, &mut letters, &mut chars)
        .then(|| String::from_utf8_lossy(&letters).into_owned())
}

/// 読みの綴りの流儀ごとのキー経路。ヘボン式が代表の綴りと同じ経路になる読みは 1 つだけ返す
pub fn key_paths(reading: &str) -> Vec<(Style, KeyPath)> {
    let Some(canon) = key_path(reading, Style::Canon) else {
        return Vec::new();
    };
    let mut out = vec![(Style::Canon, canon)];
    if let Some(hep) = key_path(reading, Style::Hepburn)
        && hep != out[0].1
    {
        out.push((Style::Hepburn, hep));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(r: &str) -> String {
        spelling_of(r, Style::Canon).expect("綴りにできる")
    }

    fn hep(r: &str) -> String {
        spelling_of(r, Style::Hepburn).expect("綴りにできる")
    }

    fn letters(path: &KeyPath) -> String {
        path.keys.iter().map(|k| (b'a' + k) as char).collect()
    }

    #[test]
    fn canonical_and_hepburn_spellings() {
        assert_eq!(canon("しんぶん"), "sinbun");
        assert_eq!(hep("しんぶん"), "shinbun");
        assert_eq!(canon("ちゃ"), "tya");
        assert_eq!(hep("ちゃ"), "cha");
        assert_eq!(canon("ふじさん"), "huzisan");
        assert_eq!(hep("ふじさん"), "fujisan");
        assert_eq!(canon("きょう"), "kyou");
    }

    #[test]
    fn hatsuon_doubles_before_vowel_and_y() {
        assert_eq!(canon("かんい"), "kanni");
        assert_eq!(canon("ほんや"), "honnya");
        assert_eq!(canon("かんなに"), "kannnani");
        // 語末・子音の前は 1 つ
        assert_eq!(canon("かん"), "kan");
        assert_eq!(canon("かんた"), "kanta");
    }

    #[test]
    fn sokuon_repeats_the_consonant() {
        assert_eq!(canon("きっぷ"), "kippu");
        assert_eq!(canon("がっこう"), "gakkou");
        assert_eq!(hep("まっちゃ"), "maccha");
        assert_eq!(canon("あっ"), "axtu");
    }

    #[test]
    fn long_vowel_mark_is_skipped() {
        assert_eq!(canon("こーひー"), "kohi");
        let path = key_path("こーひー", Style::Canon).unwrap();
        assert_eq!(letters(&path), "kohi");
    }

    #[test]
    fn repeated_keys_collapse_with_a_loop_mark() {
        let p = key_path("きっぷ", Style::Canon).unwrap();
        assert_eq!(letters(&p), "kipu");
        assert_eq!(p.loops, vec![false, false, true, false]);
        let p = key_path("おおきい", Style::Canon).unwrap();
        assert_eq!(letters(&p), "oki");
        assert_eq!(p.loops, vec![true, false, true]);
        assert!(p.has_loops());
        assert!(!key_path("かんた", Style::Canon).unwrap().has_loops());
    }

    #[test]
    fn rejects_non_hiragana_and_single_key_words() {
        assert!(key_path("漢字", Style::Canon).is_none());
        assert!(key_path("アイ", Style::Canon).is_none());
        assert!(key_path("a1", Style::Canon).is_none());
        // キーが 1 つ（タップと区別できない）
        assert!(key_path("あ", Style::Canon).is_none());
        assert!(key_path("ああ", Style::Canon).is_none());
        assert!(key_path("", Style::Canon).is_none());
    }

    #[test]
    fn key_paths_returns_hepburn_only_when_different() {
        let two = key_paths("ふじさん");
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].1.first(), Some(b'h' - b'a'));
        assert_eq!(two[1].1.first(), Some(b'f' - b'a'));
        assert_eq!(key_paths("かんじ").len(), 2);
        assert_eq!(key_paths("さくら").len(), 1);
    }
}
