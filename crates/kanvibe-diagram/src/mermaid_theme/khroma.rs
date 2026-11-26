//! Mermaid 테마가 쓰는 khroma 2.1.0 색 함수.
//!
//! 테마 변수는 글자로 오가고 다음 계산이 그 글자를 다시 읽으므로, 계산뿐 아니라 khroma가 남기는 글자 모양
//! (`hsl(…)`의 소수 10자리 반올림, 바뀌지 않은 색은 원래 글자 그대로)까지 같아야 파생 색이 Mermaid와 맞는다.
//! khroma의 채널 객체는 RGB와 HSL 중 읽힌 쪽만 채우고 나머지를 그때그때 계산한다. [`Channels`]가 그 게으른 계산을 따른다.
//! khroma가 예외를 던지는 입력(읽지 못하는 색, RGB와 HSL을 함께 바꾸기)은 [`KhromaError`]다. Mermaid는 이때 그리기에 실패한다.

use super::JsValue;
use super::js_number::{js_round, number_to_js_string};
use crate::sequence::css_color::named_color;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("테마 색 계산 실패: {0}")]
pub struct KhromaError(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelKind {
    All,
    Rgb,
    Hsl,
}

/// khroma `Channels`. `data`에 없는 채널은 읽을 때 다른 표현에서 계산한다
#[derive(Clone, Debug)]
struct Channels {
    red: Option<f64>,
    green: Option<f64>,
    blue: Option<f64>,
    hue: Option<f64>,
    saturation: Option<f64>,
    lightness: Option<f64>,
    alpha: f64,
    kind: ChannelKind,
    changed: bool,
    /// 읽은 원래 글자. 바뀌지 않았으면 stringify가 그대로 돌려준다
    source: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Red,
    Green,
    Blue,
    Hue,
    Saturation,
    Lightness,
    Alpha,
}

impl Channel {
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "r" => Self::Red,
            "g" => Self::Green,
            "b" => Self::Blue,
            "h" => Self::Hue,
            "s" => Self::Saturation,
            "l" => Self::Lightness,
            "a" => Self::Alpha,
            _ => return None,
        })
    }

    /// `Channel.clamp`
    fn clamp(self, value: f64) -> f64 {
        let clamp_between = |upper: f64| {
            if value >= upper {
                upper
            } else if value < 0.0 {
                0.0
            } else {
                value
            }
        };
        match self {
            Self::Red | Self::Green | Self::Blue => clamp_between(255.0),
            Self::Hue => value % 360.0,
            Self::Saturation | Self::Lightness => clamp_between(100.0),
            Self::Alpha => clamp_between(1.0),
        }
    }
}

impl Channels {
    fn from_rgb(red: f64, green: f64, blue: f64, alpha: f64, source: Option<String>) -> Self {
        Self {
            red: Some(red),
            green: Some(green),
            blue: Some(blue),
            hue: None,
            saturation: None,
            lightness: None,
            alpha,
            kind: ChannelKind::All,
            changed: false,
            source,
        }
    }

    fn from_hsl(hue: f64, saturation: f64, lightness: f64, alpha: f64, source: String) -> Self {
        Self {
            red: None,
            green: None,
            blue: None,
            hue: Some(hue),
            saturation: Some(saturation),
            lightness: Some(lightness),
            alpha,
            kind: ChannelKind::All,
            changed: false,
            source: Some(source),
        }
    }

    fn ensure_hsl(&mut self) {
        if self.hue.is_none() {
            self.hue = Some(self.rgb_to_hsl(Channel::Hue));
        }
        if self.saturation.is_none() {
            self.saturation = Some(self.rgb_to_hsl(Channel::Saturation));
        }
        if self.lightness.is_none() {
            self.lightness = Some(self.rgb_to_hsl(Channel::Lightness));
        }
    }

    fn ensure_rgb(&mut self) {
        if self.red.is_none() {
            self.red = Some(self.hsl_to_rgb(Channel::Red));
        }
        if self.green.is_none() {
            self.green = Some(self.hsl_to_rgb(Channel::Green));
        }
        if self.blue.is_none() {
            self.blue = Some(self.hsl_to_rgb(Channel::Blue));
        }
    }

    /// 채널 getter
    fn get(&mut self, channel: Channel) -> f64 {
        match channel {
            Channel::Alpha => self.alpha,
            Channel::Red | Channel::Green | Channel::Blue => {
                let stored = match channel {
                    Channel::Red => self.red,
                    Channel::Green => self.green,
                    _ => self.blue,
                };
                if let Some(value) = stored.filter(|_| self.kind != ChannelKind::Hsl) {
                    return value;
                }
                self.ensure_hsl();
                self.hsl_to_rgb(channel)
            }
            Channel::Hue | Channel::Saturation | Channel::Lightness => {
                let stored = match channel {
                    Channel::Hue => self.hue,
                    Channel::Saturation => self.saturation,
                    _ => self.lightness,
                };
                if let Some(value) = stored.filter(|_| self.kind != ChannelKind::Rgb) {
                    return value;
                }
                self.ensure_rgb();
                self.rgb_to_hsl(channel)
            }
        }
    }

    /// 채널 setter. RGB와 HSL을 함께 바꾸면 khroma처럼 실패한다
    fn set(&mut self, channel: Channel, value: f64) -> Result<(), KhromaError> {
        let kind = match channel {
            Channel::Red | Channel::Green | Channel::Blue => Some(ChannelKind::Rgb),
            Channel::Hue | Channel::Saturation | Channel::Lightness => Some(ChannelKind::Hsl),
            Channel::Alpha => None,
        };
        if let Some(kind) = kind {
            if self.kind != ChannelKind::All && self.kind != kind {
                return Err(KhromaError(
                    "Cannot change both RGB and HSL channels at the same time".to_string(),
                ));
            }
            self.kind = kind;
        }
        self.changed = true;
        let slot = match channel {
            Channel::Red => &mut self.red,
            Channel::Green => &mut self.green,
            Channel::Blue => &mut self.blue,
            Channel::Hue => &mut self.hue,
            Channel::Saturation => &mut self.saturation,
            Channel::Lightness => &mut self.lightness,
            Channel::Alpha => {
                self.alpha = value;
                return Ok(());
            }
        };
        *slot = Some(value);
        Ok(())
    }

    /// `Channel.hsl2rgb`. 없는 HSL 값은 JS처럼 NaN이 된다
    fn hsl_to_rgb(&self, channel: Channel) -> f64 {
        let hue = self.hue.unwrap_or(f64::NAN);
        let saturation = self.saturation.unwrap_or(f64::NAN);
        let lightness = self.lightness.unwrap_or(f64::NAN);
        if saturation == 0.0 || saturation.is_nan() {
            return lightness * 2.55;
        }
        let hue = hue / 360.0;
        let saturation = saturation / 100.0;
        let lightness = lightness / 100.0;
        let q = if lightness < 0.5 {
            lightness * (1.0 + saturation)
        } else {
            lightness + saturation - lightness * saturation
        };
        let p = 2.0 * lightness - q;
        let offset = match channel {
            Channel::Red => 1.0 / 3.0,
            Channel::Green => 0.0,
            _ => -1.0 / 3.0,
        };
        hue_to_rgb(p, q, hue + offset) * 255.0
    }

    /// `Channel.rgb2hsl`
    fn rgb_to_hsl(&self, channel: Channel) -> f64 {
        let red = self.red.unwrap_or(f64::NAN) / 255.0;
        let green = self.green.unwrap_or(f64::NAN) / 255.0;
        let blue = self.blue.unwrap_or(f64::NAN) / 255.0;
        let max = js_max(&[red, green, blue]);
        let min = js_min(&[red, green, blue]);
        let lightness = (max + min) / 2.0;
        if channel == Channel::Lightness {
            return lightness * 100.0;
        }
        if max == min {
            return 0.0;
        }
        let delta = max - min;
        let saturation = if lightness > 0.5 {
            delta / (2.0 - max - min)
        } else {
            delta / (max + min)
        };
        if channel == Channel::Saturation {
            return saturation * 100.0;
        }
        if max == red {
            ((green - blue) / delta + if green < blue { 6.0 } else { 0.0 }) * 60.0
        } else if max == green {
            ((blue - red) / delta + 2.0) * 60.0
        } else if max == blue {
            ((red - green) / delta + 4.0) * 60.0
        } else {
            -1.0
        }
    }

    /// `Color.stringify`
    fn stringify(&mut self) -> String {
        if let Some(source) = self.source.clone().filter(|_| !self.changed) {
            return source;
        }
        if self.kind == ChannelKind::Hsl || self.red.is_none() {
            return self.stringify_hsl();
        }
        let channels = [
            self.get(Channel::Red),
            self.get(Channel::Green),
            self.get(Channel::Blue),
        ];
        if self.alpha < 1.0
            || channels
                .iter()
                .any(|value| value.fract() != 0.0 || !value.is_finite())
        {
            return self.stringify_rgb();
        }
        self.stringify_hex()
    }

    fn stringify_hsl(&mut self) -> String {
        let hue = lang_round(self.get(Channel::Hue));
        let saturation = lang_round(self.get(Channel::Saturation));
        let lightness = lang_round(self.get(Channel::Lightness));
        let [hue, saturation, lightness] = [hue, saturation, lightness].map(number_to_js_string);
        if self.alpha < 1.0 {
            let alpha = number_to_js_string(self.alpha);
            format!("hsla({hue}, {saturation}%, {lightness}%, {alpha})")
        } else {
            format!("hsl({hue}, {saturation}%, {lightness}%)")
        }
    }

    fn stringify_rgb(&mut self) -> String {
        let [red, green, blue] = [Channel::Red, Channel::Green, Channel::Blue]
            .map(|channel| number_to_js_string(lang_round(self.get(channel))));
        if self.alpha < 1.0 {
            let alpha = number_to_js_string(lang_round(self.alpha));
            format!("rgba({red}, {green}, {blue}, {alpha})")
        } else {
            format!("rgb({red}, {green}, {blue})")
        }
    }

    fn stringify_hex(&mut self) -> String {
        let mut text = String::from("#");
        for channel in [Channel::Red, Channel::Green, Channel::Blue] {
            text.push_str(&dec_to_hex(self.get(channel)));
        }
        if self.alpha < 1.0 {
            text.push_str(&dec_to_hex(self.alpha * 255.0));
        }
        text
    }
}

/// `Channel.hue2rgb`
fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// `Math.max`: NaN이 하나라도 있으면 NaN
fn js_max(values: &[f64]) -> f64 {
    if values.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

fn js_min(values: &[f64]) -> f64 {
    if values.iter().any(|value| value.is_nan()) {
        return f64::NAN;
    }
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

/// `Lang.round`: 소수 10자리
fn lang_round(value: f64) -> f64 {
    js_round(value * 1e10) / 1e10
}

/// `DEC2HEX[Math.round(value)]`. 표에 없는 값은 JS처럼 `undefined`가 글자로 들어간다
fn dec_to_hex(value: f64) -> String {
    let rounded = js_round(value);
    if !(0.0..=255.0).contains(&rounded) {
        return "undefined".to_string();
    }
    format!("{:02x}", rounded as u8)
}

/// `Color.parse`: hex → rgb() → hsl() → 이름 순서
fn parse(color: &JsValue) -> Result<Channels, KhromaError> {
    let JsValue::String(text) = color else {
        return Err(KhromaError(format!(
            "Unsupported color format: \"{}\"",
            color.to_property_key()
        )));
    };
    parse_hex(text, text)
        .or_else(|| parse_rgb(text))
        .or_else(|| parse_hsl(text))
        .or_else(|| parse_keyword(text))
        .ok_or_else(|| KhromaError(format!("Unsupported color format: \"{text}\"")))
}

/// `Hex.parse`: `#rgb`·`#rgba`·`#rrggbb`·`#rrggbbaa`
fn parse_hex(text: &str, source: &str) -> Option<Channels> {
    let digits = text.strip_prefix('#')?;
    let length = digits.len();
    if !matches!(length, 3 | 4 | 6 | 8) || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    let has_alpha = length % 4 == 0;
    let is_full_length = length > 4;
    let multiplier = if is_full_length { 1.0 } else { 17.0 };
    let bits = if is_full_length { 8 } else { 4 };
    let offset = if has_alpha { 0 } else { -1 };
    let mask = if is_full_length { 255 } else { 15 };
    let channel =
        |position: i32| f64::from((value >> (bits * (offset + position))) & mask) * multiplier;
    let alpha = if has_alpha {
        f64::from(value & mask) * multiplier / 255.0
    } else {
        1.0
    };
    Some(Channels::from_rgb(
        channel(3),
        channel(2),
        channel(1),
        alpha,
        Some(source.to_string()),
    ))
}

/// 정규식 `-?(?:\d+(?:\.\d+)?|(?:\.\d+))(?:e-?\d+)?`의 숫자 하나를 읽는다. `allow_negative_exponent`는 hsl()만 켠다
fn read_number(text: &str, allow_negative_exponent: bool) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    let digits_from = |start: usize| {
        bytes[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let integer = digits_from(index);
    index += integer;
    if bytes.get(index) == Some(&b'.') && digits_from(index + 1) > 0 {
        index += 1 + digits_from(index + 1);
    } else if integer == 0 {
        return None;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let mut exponent = index + 1;
        if allow_negative_exponent && bytes.get(exponent) == Some(&b'-') {
            exponent += 1;
        }
        let exponent_digits = digits_from(exponent);
        if exponent_digits > 0 {
            index = exponent + exponent_digits;
        }
    }
    Some((&text[..index], &text[index..]))
}

fn skip_whitespace(text: &str) -> &str {
    text.trim_start_matches(|character: char| character.is_whitespace())
}

/// `\s*?(?:,|\s)\s*?`: 쉼표나 공백 하나 이상
fn read_separator(text: &str) -> Option<&str> {
    let trimmed = skip_whitespace(text);
    if let Some(rest) = trimmed.strip_prefix(',') {
        return Some(skip_whitespace(rest));
    }
    (trimmed.len() < text.len()).then_some(trimmed)
}

/// `rgb(`·`hsla(` 같은 머리를 대소문자 없이 걷는다
fn strip_function<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let head = text.get(..name.len())?;
    if !head.eq_ignore_ascii_case(name) {
        return None;
    }
    let rest = &text[name.len()..];
    let rest = match rest.get(..1) {
        Some(letter) if letter.eq_ignore_ascii_case("a") => &rest[1..],
        _ => rest,
    };
    rest.strip_prefix('(')
}

/// 함수 표기의 네 번째 값(`, a`나 `/ a`)과 닫는 괄호
fn read_alpha_and_close(text: &str, allow_negative_exponent: bool) -> Option<Option<(f64, bool)>> {
    let trimmed = skip_whitespace(text);
    if let Some(rest) = trimmed.strip_prefix(')') {
        return rest.is_empty().then_some(None);
    }
    let rest = trimmed.strip_prefix([',', '/'])?;
    let rest = skip_whitespace(rest);
    let rest = rest.strip_prefix('+').unwrap_or(rest);
    let (number, rest) = read_number(rest, allow_negative_exponent)?;
    let (is_percentage, rest) = match rest.strip_prefix('%') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let rest = skip_whitespace(rest).strip_prefix(')')?;
    rest.is_empty()
        .then_some(Some((number.parse().ok()?, is_percentage)))
}

/// `RGB.parse`
fn parse_rgb(text: &str) -> Option<Channels> {
    let mut rest = skip_whitespace(strip_function(text, "rgb")?);
    let mut values = [0.0; 3];
    for (position, value) in values.iter_mut().enumerate() {
        if position > 0 {
            rest = read_separator(rest)?;
        }
        let (number, after) = read_number(rest, false)?;
        let number: f64 = number.parse().ok()?;
        rest = after;
        *value = match rest.strip_prefix('%') {
            Some(after) => {
                rest = after;
                number * 2.55
            }
            None => number,
        };
    }
    let alpha = read_alpha_value(read_alpha_and_close(rest, false)?);
    let [red, green, blue] = [
        (Channel::Red, values[0]),
        (Channel::Green, values[1]),
        (Channel::Blue, values[2]),
    ]
    .map(|(channel, value)| channel.clamp(value));
    Some(Channels::from_rgb(
        red,
        green,
        blue,
        alpha,
        Some(text.to_string()),
    ))
}

fn read_alpha_value(alpha: Option<(f64, bool)>) -> f64 {
    match alpha {
        Some((value, true)) => Channel::Alpha.clamp(value / 100.0),
        Some((value, false)) => Channel::Alpha.clamp(value),
        None => 1.0,
    }
}

/// `HSL.parse`. 채도·밝기는 `%`가 있어야 한다
fn parse_hsl(text: &str) -> Option<Channels> {
    let rest = skip_whitespace(strip_function(text, "hsl")?);
    let (hue_number, rest) = read_number(rest, true)?;
    let (hue, rest) = read_hue_unit(hue_number.parse().ok()?, rest);
    let rest = read_separator(rest)?;
    let (saturation, rest) = read_number(rest, true)?;
    let rest = read_separator(rest.strip_prefix('%')?)?;
    let (lightness, rest) = read_number(rest, true)?;
    let rest = rest.strip_prefix('%')?;
    let alpha = read_alpha_value(read_alpha_and_close(rest, true)?);
    Some(Channels::from_hsl(
        hue,
        Channel::Saturation.clamp(saturation.parse().ok()?),
        Channel::Lightness.clamp(lightness.parse().ok()?),
        alpha,
        text.to_string(),
    ))
}

/// `HSL._hue2deg`
fn read_hue_unit(number: f64, rest: &str) -> (f64, &str) {
    // 정규식은 단위를 대소문자 없이 받지만, 변환하는 switch는 소문자 단위만 맞춘다
    for unit in ["deg", "grad", "rad", "turn"] {
        let Some(written) = rest.get(..unit.len()) else {
            continue;
        };
        if !written.eq_ignore_ascii_case(unit) {
            continue;
        }
        let degrees = match written {
            "grad" => number * 0.9,
            "rad" => number * 180.0 / std::f64::consts::PI,
            "turn" => number * 360.0,
            _ => number,
        };
        return (Channel::Hue.clamp(degrees), &rest[unit.len()..]);
    }
    (Channel::Hue.clamp(number), rest)
}

/// `Keyword.parse`. khroma의 이름표는 CSS 이름 색과 거의 같지만 `cyan`·`steelblue`·`tomato`가 없고
/// (`cyan` 자리에 `cyanaqua`가 있다) `transparent`가 있다. 결과의 원래 글자는 이름이 아니라 hex 값이다
fn parse_keyword(text: &str) -> Option<Channels> {
    let lower = text.to_lowercase();
    let hex = match lower.as_str() {
        "transparent" => "#00000000".to_string(),
        "cyanaqua" => "#00ffff".to_string(),
        "cyan" | "steelblue" | "tomato" => return None,
        name => format!("#{:06x}", named_color(name)?),
    };
    parse_hex(&hex, &hex)
}

fn argument(arguments: &[&JsValue], index: usize) -> JsValue {
    arguments
        .get(index)
        .map_or(JsValue::Undefined, |value| (*value).clone())
}

/// `change(color, channels)`
fn change(color: &JsValue, changes: &[(Channel, f64)]) -> Result<JsValue, KhromaError> {
    let mut channels = parse(color)?;
    for &(channel, value) in changes {
        channels.set(channel, channel.clamp(value))?;
    }
    Ok(JsValue::String(channels.stringify()))
}

/// `adjust(color, channels)`: 0이 아닌 채널 값만 더한다
pub(super) fn adjust(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let color = argument(arguments, 0);
    let mut channels = parse(&color)?;
    let mut changes = Vec::new();
    if let JsValue::Object(amounts) = argument(arguments, 1) {
        for (key, amount) in &amounts {
            let Some(channel) = Channel::from_key(key) else {
                continue;
            };
            if !amount.is_truthy() {
                continue;
            }
            changes.push((channel, channels.get(channel) + amount.to_number()));
        }
    }
    change(&color, &changes)
}

/// `adjustChannel(color, channel, amount)`
fn adjust_channel(color: &JsValue, channel: Channel, amount: f64) -> Result<JsValue, KhromaError> {
    let mut channels = parse(color)?;
    let current = channels.get(channel);
    let next = channel.clamp(current + amount);
    // JS `!==`: NaN끼리도 다르다
    if current != next || current.is_nan() {
        channels.set(channel, next)?;
    }
    Ok(JsValue::String(channels.stringify()))
}

pub(super) fn lighten(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let amount = argument(arguments, 1).to_number();
    adjust_channel(&argument(arguments, 0), Channel::Lightness, amount)
}

pub(super) fn darken(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let amount = argument(arguments, 1).to_number();
    adjust_channel(&argument(arguments, 0), Channel::Lightness, -amount)
}

/// `transparentize(color, amount)`
pub fn transparentize(color: &str, amount: f64) -> Result<String, KhromaError> {
    let value = adjust_channel(&JsValue::from(color), Channel::Alpha, -amount)?;
    Ok(value.to_property_key())
}

/// `rgba(r, g, b = 0, a = 1)`. 첫 값이 숫자가 아니면 `change(color, { a })`
pub(super) fn rgba(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let first = argument(arguments, 0);
    if !matches!(first, JsValue::Number(_)) {
        return change(
            &first,
            &[(Channel::Alpha, argument(arguments, 1).to_number())],
        );
    }
    let number_or = |index: usize, fallback: f64| match argument(arguments, index) {
        JsValue::Undefined => fallback,
        value => value.to_number(),
    };
    Ok(JsValue::String(rgba_numbers(
        first.to_number(),
        number_or(1, f64::NAN),
        number_or(2, 0.0),
        number_or(3, 1.0),
    )))
}

fn rgba_numbers(red: f64, green: f64, blue: f64, alpha: f64) -> String {
    let mut channels = Channels::from_rgb(
        Channel::Red.clamp(red),
        Channel::Green.clamp(green),
        Channel::Blue.clamp(blue),
        Channel::Alpha.clamp(alpha),
        None,
    );
    channels.stringify()
}

/// `invert(color)`: 반전한 색과 원래 색을 100:0으로 섞는다(`mix`)
pub(super) fn invert(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let color = argument(arguments, 0);
    let weight = match argument(arguments, 1) {
        JsValue::Undefined => 100.0,
        value => value.to_number(),
    };
    let mut inverse = parse(&color)?;
    for channel in [Channel::Red, Channel::Green, Channel::Blue] {
        let inverted = 255.0 - inverse.get(channel);
        inverse.set(channel, inverted)?;
    }
    let mut original = parse(&color)?;
    Ok(JsValue::String(mix(&mut inverse, &mut original, weight)))
}

/// `mix(color1, color2, weight)`
fn mix(first: &mut Channels, second: &mut Channels, weight: f64) -> String {
    let [red1, green1, blue1, alpha1] =
        [Channel::Red, Channel::Green, Channel::Blue, Channel::Alpha]
            .map(|channel| first.get(channel));
    let [red2, green2, blue2, alpha2] =
        [Channel::Red, Channel::Green, Channel::Blue, Channel::Alpha]
            .map(|channel| second.get(channel));
    let weight_scale = weight / 100.0;
    let weight_normalized = weight_scale * 2.0 - 1.0;
    let alpha_delta = alpha1 - alpha2;
    let weight1_combined = if weight_normalized * alpha_delta == -1.0 {
        weight_normalized
    } else {
        (weight_normalized + alpha_delta) / (1.0 + weight_normalized * alpha_delta)
    };
    let weight1 = (weight1_combined + 1.0) / 2.0;
    let weight2 = 1.0 - weight1;
    rgba_numbers(
        red1 * weight1 + red2 * weight2,
        green1 * weight1 + green2 * weight2,
        blue1 * weight1 + blue2 * weight2,
        alpha1 * weight_scale + alpha2 * (1.0 - weight_scale),
    )
}

/// `isDark(color)`: 상대 휘도가 0.5보다 작다
pub(super) fn is_dark(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let mut channels = parse(&argument(arguments, 0))?;
    let to_linear = |value: f64| {
        let normalized = value / 255.0;
        // khroma는 0..1로 나누기 전 값을 0.03928과 견준다
        if value > 0.03928 {
            ((normalized + 0.055) / 1.055).powf(2.4)
        } else {
            normalized / 12.92
        }
    };
    let luminance = 0.2126 * to_linear(channels.get(Channel::Red))
        + 0.7152 * to_linear(channels.get(Channel::Green))
        + 0.0722 * to_linear(channels.get(Channel::Blue));
    Ok(JsValue::Bool(lang_round(luminance) < 0.5))
}

/// theme-helpers.js `mkBorder(color, darkMode)`
pub(super) fn mk_border(arguments: &[&JsValue]) -> Result<JsValue, KhromaError> {
    let lightness = if argument(arguments, 1).is_truthy() {
        10.0
    } else {
        -10.0
    };
    let amounts = JsValue::object(vec![
        ("s", JsValue::Number(-40.0)),
        ("l", JsValue::Number(lightness)),
    ]);
    adjust(&[&argument(arguments, 0), &amounts])
}

/// `darken(color, amount)`를 글자 색에 바로 쓴다(다이어그램 styles.js)
pub fn darken_text(color: &str, amount: f64) -> Result<String, KhromaError> {
    Ok(darken(&[&JsValue::from(color), &JsValue::Number(amount)])?.to_property_key())
}

/// `lighten(color, amount)`를 글자 색에 바로 쓴다
pub fn lighten_text(color: &str, amount: f64) -> Result<String, KhromaError> {
    Ok(lighten(&[&JsValue::from(color), &JsValue::Number(amount)])?.to_property_key())
}

/// `invert(color)`를 글자 색에 바로 쓴다
pub fn invert_text(color: &str) -> Result<String, KhromaError> {
    Ok(invert(&[&JsValue::from(color)])?.to_property_key())
}

/// `adjust(color, { h, s, l })`를 글자 색에 바로 쓴다
pub fn adjust_text(color: &str, amounts: &[(&str, f64)]) -> Result<String, KhromaError> {
    let amounts = JsValue::object(
        amounts
            .iter()
            .map(|(key, value)| (*key, JsValue::Number(*value)))
            .collect(),
    );
    Ok(adjust(&[&JsValue::from(color), &amounts])?.to_property_key())
}

/// `rgba(color, alpha)`를 글자 색에 바로 쓴다
pub fn fade_text(color: &str, alpha: f64) -> Result<String, KhromaError> {
    Ok(rgba(&[&JsValue::from(color), &JsValue::Number(alpha)])?.to_property_key())
}

/// `isDark(color)`
pub fn is_dark_text(color: &str) -> Result<bool, KhromaError> {
    Ok(is_dark(&[&JsValue::from(color)])?.is_truthy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: Result<JsValue, KhromaError>) -> String {
        value.unwrap().to_property_key()
    }

    #[test]
    fn 바뀌지_않은_색은_원래_글자를_돌려준다() {
        assert_eq!(
            text(lighten(&[&JsValue::from("#ECECFF"), &JsValue::Number(0.0)])),
            "#ECECFF"
        );
        assert_eq!(
            text(adjust(&[&JsValue::from("white"), &JsValue::object(vec![])])),
            "#ffffff"
        );
    }

    #[test]
    fn khroma와_같은_글자를_만든다() {
        let amounts = JsValue::object(vec![("h", JsValue::Number(-160.0))]);
        assert_eq!(
            text(adjust(&[&JsValue::from("#ECECFF"), &amounts])),
            "hsl(80, 100%, 96.2745098039%)"
        );
        assert_eq!(text(invert(&[&JsValue::from("#333")])), "#cccccc");
        assert_eq!(
            text(rgba(&[
                &JsValue::Number(255.0),
                &JsValue::Number(255.0),
                &JsValue::Number(255.0),
                &JsValue::Number(0.25)
            ])),
            "rgba(255, 255, 255, 0.25)"
        );
        assert_eq!(
            text(lighten(&[&JsValue::from("#ECECFF")])),
            "hsl(240, 100%, NaN%)"
        );
    }

    #[test]
    fn 읽지_못하는_색은_실패한다() {
        assert!(lighten(&[&JsValue::from("steelblue"), &JsValue::Number(5.0)]).is_err());
        assert!(lighten(&[&JsValue::Undefined, &JsValue::Number(5.0)]).is_err());
        assert_eq!(number_to_js_string(f64::NAN), "NaN");
    }
}
