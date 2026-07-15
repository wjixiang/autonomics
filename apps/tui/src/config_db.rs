//! CRUD access for the `providers` and `models` tables backing the Config tab.
//!
//! The schema is created in `app.rs::init_database`. Both tables use an
//! auto-incrementing integer primary key (`id`).

use rusqlite::Connection;

/// A row from the `providers` table.
#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub auth_method: String,
}

/// A row from the `models` table.
#[derive(Debug, Clone)]
pub struct ModelRow {
    pub id: i64,
    pub model_name: String,
    pub provider_id: i64,
    pub context_length: i64,
    pub max_output_tokens: i64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

impl ProviderRow {
    pub fn all(conn: &Connection) -> rusqlite::Result<Vec<ProviderRow>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, base_url, api_key, auth_method FROM providers ORDER BY id",
        )?;
        stmt.query_map([], |row| {
            Ok(ProviderRow {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_type: row.get(2)?,
                base_url: row.get(3)?,
                api_key: row.get(4)?,
                auth_method: row.get(5)?,
            })
        })?
        .collect()
    }

    pub fn insert(conn: &Connection, p: &ProviderInput) -> rusqlite::Result<i64> {
        conn.execute(
            "INSERT INTO providers (name, provider_type, base_url, api_key, auth_method)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &p.name,
                &p.provider_type,
                &p.base_url,
                &p.api_key,
                &p.auth_method,
            ),
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update(conn: &Connection, id: i64, p: &ProviderInput) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE providers
                SET name = ?1, provider_type = ?2, base_url = ?3, api_key = ?4, auth_method = ?5
              WHERE id = ?6",
            (
                &p.name,
                &p.provider_type,
                &p.base_url,
                &p.api_key,
                &p.auth_method,
                id,
            ),
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
        conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
        Ok(())
    }
}

/// Flattened, stringly-typed provider values used by the edit form.
pub struct ProviderInput {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub auth_method: String,
}

impl ProviderInput {
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            provider_type: "Anthropic".to_string(),
            base_url: String::new(),
            api_key: String::new(),
            auth_method: "Anthropic".to_string(),
        }
    }
}

impl ModelRow {
    pub fn all(conn: &Connection) -> rusqlite::Result<Vec<ModelRow>> {
        let mut stmt = conn.prepare(
            "SELECT id, model_name, provider_id, context_length, max_output_tokens,
                    vision_ability, supports_function_calling, supports_streaming,
                    supports_thinking, input_token_price, output_token_price
               FROM models ORDER BY id",
        )?;
        stmt.query_map([], |row| {
            Ok(ModelRow {
                id: row.get(0)?,
                model_name: row.get(1)?,
                provider_id: row.get(2)?,
                context_length: row.get(3)?,
                max_output_tokens: row.get(4)?,
                vision_ability: row.get::<_, i32>(5)? != 0,
                supports_function_calling: row.get::<_, i32>(6)? != 0,
                supports_streaming: row.get::<_, i32>(7)? != 0,
                supports_thinking: row.get::<_, i32>(8)? != 0,
                input_token_price: row.get(9)?,
                output_token_price: row.get(10)?,
            })
        })?
        .collect()
    }

    pub fn insert(conn: &Connection, m: &ModelInput) -> rusqlite::Result<i64> {
        conn.execute(
            "INSERT INTO models (
                model_name, provider_id, context_length, max_output_tokens,
                vision_ability, supports_function_calling, supports_streaming,
                supports_thinking, input_token_price, output_token_price
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &m.model_name,
                m.provider_id,
                m.context_length,
                m.max_output_tokens,
                m.vision_ability as i32,
                m.supports_function_calling as i32,
                m.supports_streaming as i32,
                m.supports_thinking as i32,
                m.input_token_price,
                m.output_token_price,
            ),
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update(conn: &Connection, id: i64, m: &ModelInput) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE models
                SET model_name = ?1, provider_id = ?2, context_length = ?3,
                    max_output_tokens = ?4, vision_ability = ?5,
                    supports_function_calling = ?6, supports_streaming = ?7,
                    supports_thinking = ?8, input_token_price = ?9,
                    output_token_price = ?10
              WHERE id = ?11",
            (
                &m.model_name,
                m.provider_id,
                m.context_length,
                m.max_output_tokens,
                m.vision_ability as i32,
                m.supports_function_calling as i32,
                m.supports_streaming as i32,
                m.supports_thinking as i32,
                m.input_token_price,
                m.output_token_price,
                id,
            ),
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
        conn.execute("DELETE FROM models WHERE id = ?1", [id])?;
        Ok(())
    }
}

/// Flattened model values used by the edit form.
pub struct ModelInput {
    pub model_name: String,
    pub provider_id: i64,
    pub context_length: i64,
    pub max_output_tokens: i64,
    pub vision_ability: bool,
    pub supports_function_calling: bool,
    pub supports_streaming: bool,
    pub supports_thinking: bool,
    pub input_token_price: f64,
    pub output_token_price: f64,
}

impl ModelInput {
    pub fn empty() -> Self {
        Self {
            model_name: String::new(),
            provider_id: 0,
            context_length: 0,
            max_output_tokens: 0,
            vision_ability: false,
            supports_function_calling: true,
            supports_streaming: true,
            supports_thinking: false,
            input_token_price: 0.0,
            output_token_price: 0.0,
        }
    }
}

// ── ConfigTabState persistence ────────────────────────────
//
// Save / delete / reload methods that operate on `ConfigTabState`
// using the database. Extracted from `app.rs` to keep persistence
// logic next to the data model.

use crate::state::{ConfigMode, ConfigPane, ConfigTabState, ModelForm, ProviderForm};

/// Re-read providers and models from the database, clamping selections.
pub fn reload_config(cs: &mut ConfigTabState, conn: &Connection) {
    cs.providers = ProviderRow::all(conn).unwrap_or_default();
    cs.models = ModelRow::all(conn).unwrap_or_default();
    if !cs.providers.is_empty() && cs.selected_provider >= cs.providers.len() {
        cs.selected_provider = cs.providers.len() - 1;
    } else if cs.providers.is_empty() {
        cs.selected_provider = 0;
    }
    if !cs.models.is_empty() && cs.selected_model >= cs.models.len() {
        cs.selected_model = cs.models.len() - 1;
    } else if cs.models.is_empty() {
        cs.selected_model = 0;
    }
}

/// Validate, persist, and reload after saving a provider form.
/// Returns `Ok(message)` on success or `Err(message)` on failure.
pub fn save_provider(cs: &mut ConfigTabState, conn: &Connection) -> Result<String, String> {
    let form = match std::mem::replace(&mut cs.mode, ConfigMode::Browsing) {
        ConfigMode::EditProvider(f) => f,
        _ => return Ok(String::new()),
    };

    match form.collect() {
        Err(e) => {
            cs.message = e.clone();
            cs.mode = ConfigMode::EditProvider(form);
            Err(e)
        }
        Ok(input) => {
            let id = form.id;
            let res = match id {
                Some(id) => ProviderRow::update(conn, id, &input),
                None => ProviderRow::insert(conn, &input).map(|_| ()),
            };
            match res {
                Ok(()) => {
                    reload_config(cs, conn);
                    Ok(match id {
                        Some(_) => "provider updated".to_string(),
                        None => "provider added".to_string(),
                    })
                }
                Err(e) => {
                    cs.message = format!("db error: {e}");
                    cs.mode = ConfigMode::EditProvider(form);
                    Err(cs.message.clone())
                }
            }
        }
    }
}

/// Validate, persist, and reload after saving a model form.
/// Returns `Ok(message)` on success or `Err(message)` on failure.
pub fn save_model(cs: &mut ConfigTabState, conn: &Connection) -> Result<String, String> {
    let providers = cs.providers.clone();
    let form = match std::mem::replace(&mut cs.mode, ConfigMode::Browsing) {
        ConfigMode::EditModel(f) => f,
        _ => return Ok(String::new()),
    };

    match form.collect(&providers) {
        Err(e) => {
            cs.message = e.clone();
            cs.mode = ConfigMode::EditModel(form);
            Err(e)
        }
        Ok(input) => {
            let id = form.id;
            let res = match id {
                Some(id) => ModelRow::update(conn, id, &input),
                None => ModelRow::insert(conn, &input).map(|_| ()),
            };
            match res {
                Ok(()) => {
                    reload_config(cs, conn);
                    Ok(match id {
                        Some(_) => "model updated".to_string(),
                        None => "model added".to_string(),
                    })
                }
                Err(e) => {
                    cs.message = format!("db error: {e}");
                    cs.mode = ConfigMode::EditModel(form);
                    Err(cs.message.clone())
                }
            }
        }
    }
}

/// Delete the currently selected provider or model row and reload.
/// Returns a status message.
pub fn delete_selected(cs: &mut ConfigTabState, conn: &Connection) -> String {
    match cs.pane {
        ConfigPane::Providers => {
            let Some(row) = cs.selected_provider_row().cloned() else {
                return String::new();
            };
            match ProviderRow::delete(conn, row.id) {
                Ok(()) => {
                    reload_config(cs, conn);
                    format!("deleted provider '{}'", row.name)
                }
                Err(e) => format!("db error: {e}"),
            }
        }
        ConfigPane::Models => {
            let Some(row) = cs.selected_model_row().cloned() else {
                return String::new();
            };
            match ModelRow::delete(conn, row.id) {
                Ok(()) => {
                    reload_config(cs, conn);
                    format!("deleted model '{}'", row.model_name)
                }
                Err(e) => format!("db error: {e}"),
            }
        }
    }
}
