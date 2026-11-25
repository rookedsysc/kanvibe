//! 화면 목록의 locale 정렬(TS `String.prototype.localeCompare`, ICU 기본 세기). 한글·영문·숫자가 섞인
//! 프로젝트 이름과 작업 제목을 Chromium과 같은 순서로 늘어놓는다.

use std::cmp::Ordering;

use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale as IcuLocale;
use kanvibe_domain::i18n::Locale;

pub struct LocaleCollator {
    collator: CollatorBorrowed<'static>,
}

impl LocaleCollator {
    /// 앱 locale의 정렬 규칙. 그 locale 자료가 없으면 루트 규칙으로 정렬한다
    pub fn for_locale(locale: Locale) -> Self {
        let preferences = locale
            .as_str()
            .parse::<IcuLocale>()
            .map(|parsed| CollatorPreferences::from(&parsed))
            .unwrap_or_default();
        let collator = Collator::try_new(preferences, CollatorOptions::default())
            .or_else(|_| {
                Collator::try_new(CollatorPreferences::default(), CollatorOptions::default())
            })
            .expect("컴파일된 루트 정렬 자료는 늘 있다");
        Self { collator }
    }

    pub fn compare(&self, left: &str, right: &str) -> Ordering {
        self.collator.compare(left, right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_like_locale_compare_rather_than_by_code_point() {
        let collator = LocaleCollator::for_locale(Locale::Ko);
        assert_eq!(collator.compare("apple", "Banana"), Ordering::Less);
        assert_eq!(collator.compare("a", "A"), Ordering::Less);
        assert_eq!(collator.compare("가나", "다라"), Ordering::Less);
    }
}
