use crate::config::audit_path;
use crate::model::{ExecutionReport, Result};
use std::fs::OpenOptions;
use std::io::Write;

pub fn append_report(report: &ExecutionReport) -> Result<()> {
    let path = audit_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    let line = serde_json::to_string(report)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

pub fn read_reports(limit: usize) -> Result<Vec<ExecutionReport>> {
    let path = audit_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)?;
    let mut reports: Vec<ExecutionReport> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    // newest last; return last `limit`
    if reports.len() > limit {
        reports = reports.split_off(reports.len() - limit);
    }
    Ok(reports)
}
