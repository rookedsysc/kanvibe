//! render에서 매번 다시 만들던 파생 값을 입력 키가 바뀔 때만 만든다(TS `useMemo` 자리).
//! render는 `&self`로 읽는 경우가 많아 안쪽 가변성으로 마지막 값 하나만 기억한다.

use std::cell::RefCell;
use std::rc::Rc;

pub(crate) struct KeyedRenderCache<K, V> {
    cached: RefCell<Option<(K, Rc<V>)>>,
}

impl<K, V> Default for KeyedRenderCache<K, V> {
    fn default() -> Self {
        Self {
            cached: RefCell::new(None),
        }
    }
}

impl<K: PartialEq, V> KeyedRenderCache<K, V> {
    /// 마지막 키와 같으면 기억한 값을, 다르면 `build`로 새로 만든 값을 돌려준다
    pub(crate) fn get_or_build(&self, key: K, build: impl FnOnce() -> V) -> Rc<V> {
        if let Some((cached_key, value)) = self.cached.borrow().as_ref()
            && *cached_key == key
        {
            return Rc::clone(value);
        }
        let value = Rc::new(build());
        *self.cached.borrow_mut() = Some((key, Rc::clone(&value)));
        value
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn the_same_key_reuses_the_value_and_a_new_key_rebuilds_it() {
        let cache = KeyedRenderCache::<u64, String>::default();
        let build_count = Cell::new(0);
        let build = || {
            build_count.set(build_count.get() + 1);
            format!("build {}", build_count.get())
        };

        let first = cache.get_or_build(1, build);
        let second = cache.get_or_build(1, build);
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(build_count.get(), 1);

        assert_eq!(*cache.get_or_build(2, build), "build 2");
        assert_eq!(*cache.get_or_build(1, build), "build 3");
    }
}
