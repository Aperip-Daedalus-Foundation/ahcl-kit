use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchStatus {
    Success,
    Drift,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchValue<T> {
    pub status: BatchStatus,
    pub value: T,
}

impl<T> BatchValue<T> {
    pub fn success(value: T) -> Self {
        Self {
            status: BatchStatus::Success,
            value,
        }
    }

    pub fn drift(value: T) -> Self {
        Self {
            status: BatchStatus::Drift,
            value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchItem<T, E> {
    pub project: PathBuf,
    pub result: Result<BatchValue<T>, E>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchExecution<T, E> {
    pub items: Vec<BatchItem<T, E>>,
}

impl<T, E> BatchExecution<T, E> {
    pub fn exit_code(&self) -> u8 {
        if self.items.iter().any(|item| item.result.is_err()) {
            1
        } else if self.items.iter().any(|item| {
            item.result
                .as_ref()
                .is_ok_and(|value| value.status == BatchStatus::Drift)
        }) {
            2
        } else {
            0
        }
    }
}

pub fn execute_batch<T, E, F>(
    mut projects: Vec<PathBuf>,
    fail_fast: bool,
    mut operation: F,
) -> BatchExecution<T, E>
where
    F: FnMut(&Path) -> Result<BatchValue<T>, E>,
{
    projects.sort_by_key(|project| path_key(project));
    let mut items = Vec::new();
    for project in projects {
        let result = operation(&project);
        let failed = result.is_err();
        items.push(BatchItem { project, result });
        if failed && fail_fast {
            break;
        }
    }
    BatchExecution { items }
}

fn path_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}
