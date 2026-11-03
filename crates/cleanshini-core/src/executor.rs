use crate::model::{CoreError, DeleteMode, ExecutionRecord, ExecutionReport, Plan, PlanItem, Result};
use chrono::Utc;
use std::path::PathBuf;
use trash::delete as trash_delete;

pub fn execute_plan<F>(plan: &Plan, permanent_override: bool, mut progress: F) -> Result<ExecutionReport>
where
    F: FnMut(usize, usize, &PathBuf),
{
    let started = Utc::now();
    let selected: Vec<&PlanItem> = plan.items.iter().filter(|i| i.selected).collect();
    let total = selected.len();
    let mut records = Vec::new();
    let mut total_freed = 0u64;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for (idx, pi) in selected.iter().enumerate() {
        progress(idx, total, &pi.item.path);
        let mode = if permanent_override {
            DeleteMode::Permanent
        } else {
            pi.action
        };
        let res = delete_one(&pi.item.path, mode);
        match res {
            Ok(()) => {
                succeeded += 1;
                total_freed += pi.item.size;
                records.push(ExecutionRecord {
                    path: pi.item.path.clone(),
                    size: pi.item.size,
                    category: pi.item.category,
                    mode,
                    success: true,
                    error: None,
                    timestamp: Utc::now(),
                });
            }
            Err(e) => {
                failed += 1;
                records.push(ExecutionRecord {
                    path: pi.item.path.clone(),
                    size: pi.item.size,
                    category: pi.item.category,
                    mode,
                    success: false,
                    error: Some(e.to_string()),
                    timestamp: Utc::now(),
                });
            }
        }
    }

    Ok(ExecutionReport {
        started_at: started,
        finished_at: Utc::now(),
        records,
        total_freed,
        succeeded,
        failed,
    })
}

fn delete_one(path: &PathBuf, mode: DeleteMode) -> Result<()> {
    match mode {
        DeleteMode::RecycleBin => {
            trash_delete(path).map_err(|e| CoreError::Trash(e.to_string()))
        }
        DeleteMode::Permanent => {
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
            Ok(())
        }
    }
}
