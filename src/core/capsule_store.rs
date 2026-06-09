use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{
    Connection, OptionalExtension, Row, params,
    types::{FromSqlError, Type},
};
use serde::{Deserialize, Serialize};

use super::{
    compiler,
    error::CoreError,
    local_jsonl::stable_text_digest,
    model::{CliTool, VerificationCheck, VerificationReport, VerificationStatus, WorkCapsule},
};

const STORE_SCHEMA_VERSION: u16 = 1;
const EXPORT_VERSION: u16 = 1;
const EXPORT_KIND: &str = "moonbox.capsule.export";
const EXPORT_SOURCE: &str = "moonbox";
const CAPSULE_IMPORT_MAX_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleSummary {
    pub version: u16,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub checksum: String,
    pub size_bytes: usize,
    pub source_cli: CliTool,
    pub target_cli: CliTool,
    pub source_session: String,
    pub rewind_point: String,
    pub compiler: String,
    pub handoff_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleRecord {
    #[serde(flatten)]
    pub summary: CapsuleSummary,
    pub capsule: WorkCapsule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleExportEnvelope {
    #[serde(default)]
    pub version: u16,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub exported_by: String,
    #[serde(default)]
    pub exported_at: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub checksum: String,
    pub capsule: WorkCapsule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleImportResult {
    pub imported: bool,
    pub name: String,
    pub verification: VerificationReport,
    pub record: Option<CapsuleRecord>,
}

pub fn default_store_path() -> Result<PathBuf, CoreError> {
    if let Ok(path) = env::var("MOONBOX_CAPSULE_STORE")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }

    let home = env::var_os("HOME").ok_or_else(|| CoreError::CapsuleStore {
        reason: "HOME is not set and MOONBOX_CAPSULE_STORE was not provided".into(),
    })?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("moonbox")
        .join("capsules.sqlite"))
}

pub fn save_capsule(name: &str, capsule: &WorkCapsule) -> Result<CapsuleRecord, CoreError> {
    CapsuleStore::open_default()?.save(name, capsule)
}

pub fn list_capsules() -> Result<Vec<CapsuleSummary>, CoreError> {
    CapsuleStore::open_default()?.list()
}

pub fn show_capsule(name: &str) -> Result<Option<CapsuleRecord>, CoreError> {
    CapsuleStore::open_default()?.show(name)
}

pub fn delete_capsule(name: &str) -> Result<bool, CoreError> {
    CapsuleStore::open_default()?.delete(name)
}

pub fn export_capsule(name: &str) -> Result<CapsuleExportEnvelope, CoreError> {
    CapsuleStore::open_default()?.export(name)
}

pub fn import_capsule(
    envelope: CapsuleExportEnvelope,
    name_override: Option<&str>,
) -> Result<CapsuleImportResult, CoreError> {
    CapsuleStore::open_default()?.import(envelope, name_override)
}

pub fn read_export_file(path: &Path) -> Result<CapsuleExportEnvelope, CoreError> {
    let contents = fs::read_to_string(path).map_err(|error| CoreError::CapsuleRead {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    serde_json::from_str::<CapsuleExportEnvelope>(&contents).map_err(|error| {
        CoreError::CapsuleParse {
            path: path.display().to_string(),
            reason: error.to_string(),
        }
    })
}

pub fn write_export_file(path: &Path, envelope: &CapsuleExportEnvelope) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| CoreError::CapsuleStore {
            reason: format!(
                "cannot create export directory {}: {error}",
                parent.display()
            ),
        })?;
    }
    let contents =
        serde_json::to_string_pretty(envelope).map_err(|error| CoreError::CapsuleStore {
            reason: format!("cannot serialize capsule export: {error}"),
        })?;
    fs::write(path, contents).map_err(|error| CoreError::CapsuleStore {
        reason: format!("cannot write capsule export {}: {error}", path.display()),
    })
}

pub struct CapsuleStore {
    connection: Connection,
}

impl CapsuleStore {
    pub fn open_default() -> Result<Self, CoreError> {
        Self::open_path(default_store_path()?)
    }

    pub fn open_path(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| CoreError::CapsuleStore {
                reason: format!(
                    "cannot create capsule store directory {}: {error}",
                    parent.display()
                ),
            })?;
        }
        let connection = Connection::open(path).map_err(|error| CoreError::CapsuleStore {
            reason: format!("cannot open capsule store {}: {error}", path.display()),
        })?;
        let store = Self { connection };
        store.ensure_schema()?;
        Ok(store)
    }

    pub fn save(&self, name: &str, capsule: &WorkCapsule) -> Result<CapsuleRecord, CoreError> {
        let name = validate_name(name)?;
        let now = now_timestamp();
        let capsule_json = capsule_json(capsule)?;
        let checksum = capsule_checksum_from_json(&capsule_json);
        let size_bytes = capsule_json.len();
        let size_i64 = i64::try_from(size_bytes).map_err(|error| CoreError::CapsuleStore {
            reason: format!("capsule is too large to store: {error}"),
        })?;
        self.connection
            .execute(
                r#"
                insert into capsules (
                    name, created_at, updated_at, checksum, size_bytes,
                    source_cli, target_cli, source_session, rewind_point,
                    compiler, handoff_label, capsule_json
                ) values (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                on conflict(name) do update set
                    updated_at = excluded.updated_at,
                    checksum = excluded.checksum,
                    size_bytes = excluded.size_bytes,
                    source_cli = excluded.source_cli,
                    target_cli = excluded.target_cli,
                    source_session = excluded.source_session,
                    rewind_point = excluded.rewind_point,
                    compiler = excluded.compiler,
                    handoff_label = excluded.handoff_label,
                    capsule_json = excluded.capsule_json
                "#,
                params![
                    name,
                    now,
                    checksum,
                    size_i64,
                    capsule.source_cli.id(),
                    capsule.target_cli.id(),
                    capsule.source_session,
                    capsule.rewind_point,
                    capsule.compiler,
                    capsule.handoff_label,
                    capsule_json
                ],
            )
            .map_err(sql_error)?;
        self.show(&name)?.ok_or_else(|| CoreError::CapsuleStore {
            reason: format!("saved capsule {name} could not be read back"),
        })
    }

    pub fn list(&self) -> Result<Vec<CapsuleSummary>, CoreError> {
        let mut statement = self
            .connection
            .prepare(
                r#"
                select name, created_at, updated_at, checksum, size_bytes,
                       source_cli, target_cli, source_session, rewind_point,
                       compiler, handoff_label
                from capsules
                order by updated_at desc, name asc
                "#,
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([], summary_from_row)
            .map_err(sql_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(sql_error)
    }

    pub fn show(&self, name: &str) -> Result<Option<CapsuleRecord>, CoreError> {
        let name = validate_name(name)?;
        self.connection
            .query_row(
                r#"
                select name, created_at, updated_at, checksum, size_bytes,
                       source_cli, target_cli, source_session, rewind_point,
                       compiler, handoff_label, capsule_json
                from capsules
                where name = ?1
                "#,
                params![name],
                record_from_row,
            )
            .optional()
            .map_err(sql_error)
    }

    pub fn delete(&self, name: &str) -> Result<bool, CoreError> {
        let name = validate_name(name)?;
        let count = self
            .connection
            .execute("delete from capsules where name = ?1", params![name])
            .map_err(sql_error)?;
        Ok(count > 0)
    }

    pub fn export(&self, name: &str) -> Result<CapsuleExportEnvelope, CoreError> {
        let record = self.show(name)?.ok_or_else(|| CoreError::CapsuleStore {
            reason: format!("capsule {name} was not found"),
        })?;
        Ok(CapsuleExportEnvelope {
            version: EXPORT_VERSION,
            kind: EXPORT_KIND.into(),
            exported_by: EXPORT_SOURCE.into(),
            exported_at: now_timestamp(),
            name: record.summary.name,
            checksum: record.summary.checksum,
            capsule: record.capsule,
        })
    }

    pub fn import(
        &self,
        envelope: CapsuleExportEnvelope,
        name_override: Option<&str>,
    ) -> Result<CapsuleImportResult, CoreError> {
        let name = match name_override {
            Some(name) => validate_name(name)?,
            None => validate_name(&envelope.name)?,
        };
        let verification = validate_import_envelope(&envelope);
        if !verification.ready {
            return Ok(CapsuleImportResult {
                imported: false,
                name,
                verification,
                record: None,
            });
        }
        let record = self.save(&name, &envelope.capsule)?;
        Ok(CapsuleImportResult {
            imported: true,
            name,
            verification,
            record: Some(record),
        })
    }

    fn ensure_schema(&self) -> Result<(), CoreError> {
        self.connection
            .execute_batch(
                r#"
                create table if not exists metadata (
                    key text primary key,
                    value text not null
                );
                create table if not exists capsules (
                    name text primary key,
                    created_at text not null,
                    updated_at text not null,
                    checksum text not null,
                    size_bytes integer not null,
                    source_cli text not null,
                    target_cli text not null,
                    source_session text not null,
                    rewind_point text not null,
                    compiler text not null,
                    handoff_label text not null,
                    capsule_json text not null
                );
                "#,
            )
            .map_err(sql_error)?;
        let version = self
            .connection
            .query_row(
                "select value from metadata where key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_error)?;
        match version.as_deref() {
            Some("1") => Ok(()),
            Some(version) => Err(CoreError::CapsuleStore {
                reason: format!(
                    "unsupported capsule store schema version {version}; supported {STORE_SCHEMA_VERSION}"
                ),
            }),
            None => {
                self.connection
                    .execute(
                        "insert into metadata (key, value) values ('schema_version', ?1)",
                        params![STORE_SCHEMA_VERSION.to_string()],
                    )
                    .map_err(sql_error)?;
                Ok(())
            }
        }
    }
}

pub fn validate_import_envelope(envelope: &CapsuleExportEnvelope) -> VerificationReport {
    let checksum = capsule_json(&envelope.capsule)
        .map(|json| (capsule_checksum_from_json(&json), json.len()))
        .unwrap_or_else(|_| ("<serialize-error>".into(), usize::MAX));
    let compiler_known = compiler::compiler_catalog_entries()
        .iter()
        .any(|entry| entry.id == envelope.capsule.compiler);
    let checks = vec![
        check(
            "trusted_source",
            envelope.kind == EXPORT_KIND && envelope.exported_by == EXPORT_SOURCE,
            format!(
                "kind={} exported_by={}",
                empty_label(&envelope.kind),
                empty_label(&envelope.exported_by)
            ),
        ),
        check(
            "export_schema_version",
            envelope.version == EXPORT_VERSION,
            format!(
                "export version {} vs supported {EXPORT_VERSION}",
                envelope.version
            ),
        ),
        check(
            "capsule_schema_version",
            envelope.capsule.version == STORE_SCHEMA_VERSION,
            format!(
                "capsule version {} vs supported {STORE_SCHEMA_VERSION}",
                envelope.capsule.version
            ),
        ),
        check(
            "checksum",
            envelope.checksum == checksum.0,
            format!(
                "expected {} actual {}",
                empty_label(&envelope.checksum),
                checksum.0
            ),
        ),
        check(
            "capsule_size",
            checksum.1 <= CAPSULE_IMPORT_MAX_BYTES,
            format!("{} / {} bytes", checksum.1, CAPSULE_IMPORT_MAX_BYTES),
        ),
        check(
            "compiler_reference",
            compiler_known,
            if compiler_known {
                format!("compiler {} is known", envelope.capsule.compiler)
            } else {
                format!("compiler {} is not configured", envelope.capsule.compiler)
            },
        ),
    ];
    let status = if checks
        .iter()
        .any(|check| check.status == VerificationStatus::Fail)
    {
        VerificationStatus::Fail
    } else {
        VerificationStatus::Pass
    };
    VerificationReport {
        version: 1,
        ready: status != VerificationStatus::Fail,
        status,
        checks,
    }
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<CapsuleSummary> {
    let size_bytes = integer_to_usize(row.get::<_, i64>(4)?)?;
    Ok(CapsuleSummary {
        version: STORE_SCHEMA_VERSION,
        name: row.get(0)?,
        created_at: row.get(1)?,
        updated_at: row.get(2)?,
        checksum: row.get(3)?,
        size_bytes,
        source_cli: cli_tool_from_db(row.get::<_, String>(5)?)?,
        target_cli: cli_tool_from_db(row.get::<_, String>(6)?)?,
        source_session: row.get(7)?,
        rewind_point: row.get(8)?,
        compiler: row.get(9)?,
        handoff_label: row.get(10)?,
    })
}

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<CapsuleRecord> {
    let summary = summary_from_row(row)?;
    let capsule_json: String = row.get(11)?;
    let capsule = serde_json::from_str::<WorkCapsule>(&capsule_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(11, Type::Text, Box::new(error))
    })?;
    Ok(CapsuleRecord { summary, capsule })
}

fn capsule_json(capsule: &WorkCapsule) -> Result<String, CoreError> {
    serde_json::to_string(capsule).map_err(|error| CoreError::CapsuleStore {
        reason: format!("cannot serialize capsule: {error}"),
    })
}

fn capsule_checksum_from_json(json: &str) -> String {
    stable_text_digest(json)
}

fn validate_name(name: &str) -> Result<String, CoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::CapsuleStore {
            reason: "capsule name cannot be empty".into(),
        });
    }
    if name.len() > 96 {
        return Err(CoreError::CapsuleStore {
            reason: "capsule name must be 96 bytes or fewer".into(),
        });
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(CoreError::CapsuleStore {
            reason:
                "capsule name may only contain ASCII letters, numbers, dot, dash, and underscore"
                    .into(),
        });
    }
    Ok(name.into())
}

fn cli_tool_from_db(value: String) -> rusqlite::Result<CliTool> {
    match value.as_str() {
        "codex" => Ok(CliTool::Codex),
        "claude" => Ok(CliTool::Claude),
        "hermes" => Ok(CliTool::Hermes),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(FromSqlError::Other(Box::<
                dyn std::error::Error + Send + Sync,
            >::from(format!(
                "unknown CLI tool {value}"
            )))),
        )),
    }
}

fn integer_to_usize(value: i64) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Integer, Box::new(error))
    })
}

fn check(name: &str, passed: bool, detail: String) -> VerificationCheck {
    VerificationCheck {
        name: name.into(),
        status: if passed {
            VerificationStatus::Pass
        } else {
            VerificationStatus::Fail
        },
        detail,
    }
}

fn empty_label(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

fn now_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix:{seconds}")
}

fn sql_error(error: rusqlite::Error) -> CoreError {
    CoreError::CapsuleStore {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::core::{data, model::CliTool};

    fn store_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "moonbox-capsule-store-{name}-{}.sqlite",
            std::process::id()
        ))
    }

    fn fixture_capsule() -> WorkCapsule {
        data::fixture_workbench_data(CliTool::Codex, CliTool::Hermes)
            .expect("fixture workbench")
            .capsule
    }

    #[test]
    fn saves_lists_and_deletes_capsule_records() {
        let path = store_path("roundtrip");
        let _ = fs::remove_file(&path);
        let store = CapsuleStore::open_path(&path).expect("store");
        let capsule = fixture_capsule();

        let saved = store.save("demo", &capsule).expect("saved");
        assert_eq!(saved.summary.name, "demo");
        assert_eq!(saved.summary.source_session, "codex-cxcp-design");
        assert_eq!(saved.capsule.handoff_label, capsule.handoff_label);

        let list = store.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "demo");

        let shown = store.show("demo").expect("show").expect("record");
        assert_eq!(shown.summary.checksum, saved.summary.checksum);
        assert!(store.delete("demo").expect("delete"));
        assert!(store.list().expect("empty").is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn export_import_requires_trusted_envelope_and_checksum() {
        let path = store_path("import");
        let import_path = store_path("import-target");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&import_path);
        let store = CapsuleStore::open_path(&path).expect("store");
        let import_store = CapsuleStore::open_path(&import_path).expect("import store");
        let capsule = fixture_capsule();

        store.save("demo", &capsule).expect("saved");
        let export = store.export("demo").expect("export");
        let validation = validate_import_envelope(&export);
        assert!(validation.ready);

        let imported = import_store
            .import(export.clone(), Some("restored"))
            .expect("import");
        assert!(imported.imported);
        assert_eq!(imported.name, "restored");
        assert!(import_store.show("restored").expect("show").is_some());

        let mut tampered = export;
        tampered.checksum = "fnv64:bad".into();
        let rejected = import_store.import(tampered, Some("bad")).expect("reject");
        assert!(!rejected.imported);
        assert_eq!(rejected.verification.status, VerificationStatus::Fail);
        assert!(
            rejected
                .verification
                .checks
                .iter()
                .any(|check| check.name == "checksum" && check.status == VerificationStatus::Fail)
        );
        let _ = fs::remove_file(Path::new(&path));
        let _ = fs::remove_file(Path::new(&import_path));
    }
}
