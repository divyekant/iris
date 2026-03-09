use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FilterRule {
    pub id: String,
    pub name: String,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    pub is_active: bool,
    pub account_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Condition {
    pub field: String,    // from, to, subject, category, is_read, has_attachments
    pub operator: String, // contains, equals, starts_with, ends_with
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Action {
    #[serde(rename = "type")]
    pub action_type: String, // archive, delete, mark_read, star, label
    pub value: Option<String>,
}

/// Message data used for matching rules
pub struct MessageForMatching {
    pub from_address: String,
    pub from_name: String,
    pub to_addresses: String,
    pub subject: String,
    pub category: String,
    pub is_read: bool,
    pub has_attachments: bool,
}

pub fn create(
    conn: &Connection,
    name: &str,
    conditions: &[Condition],
    actions: &[Action],
    account_id: Option<&str>,
) -> rusqlite::Result<FilterRule> {
    let id = Uuid::new_v4().to_string();
    let cond_json = serde_json::to_string(conditions).unwrap_or_else(|_| "[]".to_string());
    let act_json = serde_json::to_string(actions).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO filter_rules (id, name, conditions, actions, account_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, cond_json, act_json, account_id],
    )?;
    get(conn, &id)
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<FilterRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, conditions, actions, is_active, account_id, created_at FROM filter_rules ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let cond_str: String = row.get(2)?;
        let act_str: String = row.get(3)?;
        Ok(FilterRule {
            id: row.get(0)?,
            name: row.get(1)?,
            conditions: serde_json::from_str(&cond_str).unwrap_or_default(),
            actions: serde_json::from_str(&act_str).unwrap_or_default(),
            is_active: row.get(4)?,
            account_id: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<FilterRule> {
    conn.query_row(
        "SELECT id, name, conditions, actions, is_active, account_id, created_at FROM filter_rules WHERE id = ?1",
        params![id],
        |row| {
            let cond_str: String = row.get(2)?;
            let act_str: String = row.get(3)?;
            Ok(FilterRule {
                id: row.get(0)?,
                name: row.get(1)?,
                conditions: serde_json::from_str(&cond_str).unwrap_or_default(),
                actions: serde_json::from_str(&act_str).unwrap_or_default(),
                is_active: row.get(4)?,
                account_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: &str,
    conditions: &[Condition],
    actions: &[Action],
    is_active: bool,
) -> rusqlite::Result<FilterRule> {
    let cond_json = serde_json::to_string(conditions).unwrap_or_else(|_| "[]".to_string());
    let act_json = serde_json::to_string(actions).unwrap_or_else(|_| "[]".to_string());
    let changed = conn.execute(
        "UPDATE filter_rules SET name = ?2, conditions = ?3, actions = ?4, is_active = ?5 WHERE id = ?1",
        params![id, name, cond_json, act_json, is_active],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute("DELETE FROM filter_rules WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

/// Check if a message matches all conditions of a rule
pub fn matches_message(conditions: &[Condition], msg: &MessageForMatching) -> bool {
    conditions.iter().all(|cond| {
        let field_value = match cond.field.as_str() {
            "from" => format!("{} {}", msg.from_address, msg.from_name),
            "to" => msg.to_addresses.clone(),
            "subject" => msg.subject.clone(),
            "category" => msg.category.clone(),
            "is_read" => if msg.is_read { "true".to_string() } else { "false".to_string() },
            "has_attachments" => if msg.has_attachments { "true".to_string() } else { "false".to_string() },
            _ => String::new(),
        };

        let field_lower = field_value.to_lowercase();
        let value_lower = cond.value.to_lowercase();

        match cond.operator.as_str() {
            "contains" => field_lower.contains(&value_lower),
            "equals" => field_lower == value_lower,
            "starts_with" => field_lower.starts_with(&value_lower),
            "ends_with" => field_lower.ends_with(&value_lower),
            _ => false,
        }
    })
}

/// Apply rule actions to a message (returns SQL updates to apply)
pub fn apply_actions(conn: &Connection, message_id: &str, actions: &[Action]) -> rusqlite::Result<()> {
    for action in actions {
        match action.action_type.as_str() {
            "archive" => {
                conn.execute(
                    "UPDATE messages SET folder = 'Archive' WHERE id = ?1",
                    params![message_id],
                )?;
            }
            "delete" => {
                conn.execute(
                    "UPDATE messages SET folder = 'Trash', is_deleted = 1 WHERE id = ?1",
                    params![message_id],
                )?;
            }
            "mark_read" => {
                conn.execute(
                    "UPDATE messages SET is_read = 1 WHERE id = ?1",
                    params![message_id],
                )?;
            }
            "star" => {
                conn.execute(
                    "UPDATE messages SET is_starred = 1 WHERE id = ?1",
                    params![message_id],
                )?;
            }
            "label" => {
                if let Some(label) = &action.value {
                    // Append label to existing labels JSON array
                    conn.execute(
                        "UPDATE messages SET labels = json_insert(COALESCE(labels, '[]'), '$[#]', ?2) WHERE id = ?1",
                        params![message_id, label],
                    )?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let schema = std::fs::read_to_string("migrations/001_initial.sql").unwrap();
        conn.execute_batch(&schema).unwrap();
        let migration = std::fs::read_to_string("migrations/016_filter_rules.sql").unwrap();
        conn.execute_batch(&migration).unwrap();
        conn
    }

    #[test]
    fn test_create_and_list() {
        let conn = setup_db();
        let conditions = vec![Condition {
            field: "from".into(),
            operator: "contains".into(),
            value: "newsletter".into(),
        }];
        let actions = vec![Action {
            action_type: "archive".into(),
            value: None,
        }];
        let rule = create(&conn, "Archive newsletters", &conditions, &actions, None).unwrap();
        assert_eq!(rule.name, "Archive newsletters");
        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.actions.len(), 1);
        assert!(rule.is_active);

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_update_and_delete() {
        let conn = setup_db();
        let rule = create(&conn, "Test", &[], &[], None).unwrap();
        let updated = update(&conn, &rule.id, "Updated", &[], &[], false).unwrap();
        assert_eq!(updated.name, "Updated");
        assert!(!updated.is_active);

        assert!(delete(&conn, &rule.id).unwrap());
        assert!(!delete(&conn, &rule.id).unwrap());
    }

    #[test]
    fn test_matches_message() {
        let conditions = vec![
            Condition { field: "from".into(), operator: "contains".into(), value: "newsletter".into() },
            Condition { field: "subject".into(), operator: "contains".into(), value: "weekly".into() },
        ];
        let msg = MessageForMatching {
            from_address: "newsletter@company.com".into(),
            from_name: "Newsletter Team".into(),
            to_addresses: "me@example.com".into(),
            subject: "Weekly Digest #42".into(),
            category: "Newsletter".into(),
            is_read: false,
            has_attachments: false,
        };
        assert!(matches_message(&conditions, &msg));

        let msg_no_match = MessageForMatching {
            from_address: "boss@company.com".into(),
            from_name: "Boss".into(),
            to_addresses: "me@example.com".into(),
            subject: "Weekly Digest".into(),
            category: "Work".into(),
            is_read: false,
            has_attachments: false,
        };
        assert!(!matches_message(&conditions, &msg_no_match));
    }
}
