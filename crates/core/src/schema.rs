use rusty_data::migration::Migration;

pub fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: 1,
        name: "billing_schema",
        sql: include_str!("../../../migrations/001_billing.sql"),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_data::migration;

    #[test]
    fn migrations_apply_cleanly() {
        let conn = rusty_data::connection::open_in_memory().unwrap();
        migration::run(&conn, &migrations()).unwrap();
        assert_eq!(migration::current_version(&conn).unwrap(), Some(1));
    }

    #[test]
    fn migrations_idempotent() {
        let conn = rusty_data::connection::open_in_memory().unwrap();
        migration::run(&conn, &migrations()).unwrap();
        migration::run(&conn, &migrations()).unwrap();
        assert_eq!(migration::current_version(&conn).unwrap(), Some(1));
    }

    #[test]
    fn all_tables_created() {
        let conn = rusty_data::connection::open_in_memory().unwrap();
        migration::run(&conn, &migrations()).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let expected = vec![
            "activity_event", "attribution_rule", "billing_project",
            "client", "connector_cursor", "decision_record",
            "time_block", "work_item",
        ];
        for t in &expected {
            assert!(tables.contains(&t.to_string()), "missing table: {t}");
        }
    }
}
