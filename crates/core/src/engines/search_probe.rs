//! Test-thread-only finite recipe-entry guard. An entry is NOT proof of process spawn.
use crate::{policy::WorkerOperation, require, Result};
use std::cell::RefCell;

#[derive(Default)]
struct State {
    expected: Vec<&'static str>,
    entered: Vec<&'static str>,
    refused: bool,
}
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
pub(crate) struct Guard;
impl Guard {
    pub(crate) fn install(expected: &[&'static str]) -> Result<Self> {
        STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            require(state.is_none(), "Nested Search campaign guard")?;
            *state = Some(State {
                expected: expected.to_vec(),
                ..State::default()
            });
            Ok(Self)
        })
    }
    pub(crate) fn entries(&self) -> Vec<&'static str> {
        STATE.with(|slot| slot.borrow().as_ref().expect("Owned guard").entered.clone())
    }
    pub(crate) fn complete(&self) -> bool {
        STATE.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|s| !s.refused && s.entered == s.expected)
        })
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        STATE.with(|slot| *slot.borrow_mut() = None);
    }
}
pub(super) fn enter(operation: &WorkerOperation) -> Result<()> {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let Some(state) = state.as_mut() else {
            return Ok(());
        };
        let label = match operation {
            WorkerOperation::Index => "index",
            WorkerOperation::Search => "search",
            _ => "unsupported",
        };
        if state.refused || state.expected.get(state.entered.len()) != Some(&label) {
            state.refused = true;
            return Err(crate::Error::Blocked(
                "Fixed Search recipe-entry guard refused".into(),
            ));
        }
        state.entered.push(label);
        Ok(())
    })
}

#[test]
fn fixed_search_guard_refuses_extra_reordered_and_any_quarantined_recipe_entry() {
    {
        let g = Guard::install(&["index", "search", "search", "search", "search"]).unwrap();
        assert!(Guard::install(&[]).is_err());
        enter(&WorkerOperation::Index).unwrap();
        for _ in 0..4 {
            enter(&WorkerOperation::Search).unwrap();
        }
        assert!(g.complete());
        assert!(enter(&WorkerOperation::Search).is_err());
        assert_eq!(g.entries().len(), 5);
        assert!(!g.complete());
    }
    {
        let g = Guard::install(&["index"]).unwrap();
        assert!(enter(&WorkerOperation::Search).is_err());
        assert!(enter(&WorkerOperation::Index).is_err());
        assert!(g.entries().is_empty());
    }
    let g = Guard::install(&[]).unwrap();
    assert!(enter(&WorkerOperation::Search).is_err());
    assert!(g.entries().is_empty());
}
