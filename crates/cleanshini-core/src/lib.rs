pub mod audit;
pub mod config;
pub mod executor;
pub mod model;
pub mod scanner;
pub mod system;

pub use model::*;
pub use config::*;
pub use scanner::scan;
pub use executor::execute_plan;
pub use audit::{append_report, read_reports};
pub use system::{empty_recycle_bin, run_system_cleanup, run_container_cleanup, list_drives, DriveInfo, recycle_bin_info, notify};
pub mod duplicates;
pub use duplicates::find_duplicates;
