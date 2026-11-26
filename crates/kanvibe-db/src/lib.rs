//! TS 앱이 만든 `kanvibe.db`를 그대로 여는 SQLite 계층.
//! TypeORM `migrations` 테이블을 읽어 이미 실행된 마이그레이션은 다시 돌리지 않고, 스키마 텍스트까지 TS 앱과 같게 만든다.

pub mod app_settings_service;
pub mod database;
pub mod error;
pub mod location;
pub mod migrations;
pub mod repositories;
pub mod schema;
mod table_introspection;

pub use database::Database;
pub use error::{DatabaseError, DatabaseResult};
pub use location::DatabaseLocation;
