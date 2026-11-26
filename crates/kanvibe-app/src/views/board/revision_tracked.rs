//! 읽기는 그대로 하고 고칠 때마다 판 번호를 올리는 값. 보드 render가 이 판 번호로 파생 값(보일 열, 카드 조회표)을 캐시한다.
//! 고치는 길을 `get_mut` 하나로 막아 두었으므로, 판 번호를 올리지 않고 값을 바꾸는 코드는 컴파일되지 않는다.

use std::ops::Deref;

pub(super) struct RevisionTracked<T> {
    value: T,
    revision: u64,
}

impl<T> RevisionTracked<T> {
    pub(super) fn new(value: T) -> Self {
        Self { value, revision: 0 }
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// 고칠 수 있는 참조. 실제로 고치지 않아도 판 번호는 오른다(파생 값을 한 번 더 만들 뿐이다)
    pub(super) fn get_mut(&mut self) -> &mut T {
        self.revision += 1;
        &mut self.value
    }
}

impl<T> Deref for RevisionTracked<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_keeps_the_revision_and_mutable_access_raises_it() {
        let mut tracked = RevisionTracked::new(vec![1]);
        assert_eq!((tracked.len(), tracked.revision()), (1, 0));
        tracked.get_mut().push(2);
        assert_eq!((tracked.len(), tracked.revision()), (2, 1));
    }
}
