//! Integration tests for schema changes made through a form, against real servers.
//! Same opt-in environment variables and disposable-database rule as `live_db.rs`: every
//! database created here is named `puricat_it_<random>…` and dropped again.

use puricat_core::db::ddl::{
    AlterDatabase, DefinitionSource, Flush, NewDatabase, TableChange, ViewSpec,
};
use puricat_core::db::{ObjectKind, Session, TableRef};
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
use serde_json::json;
use sqlx::{Connection, Executor, Row};

struct Target {
    profile: ConnectionProfile,
    password: Option<String>,
}

fn target(engine: Engine, var: &str) -> Option<Target> {
    let spec = std::env::var(var).ok()?;
    let (user, rest) = spec.split_once('@')?;
    let (host, port) = rest.rsplit_once(':')?;
    let profile = serde_json::from_value(json!({
        "name": "integration", "engine": engine, "host": host,
        "port": port.parse::<u16>().ok()?, "username": user,
    }))
    .ok()?;
    Some(Target {
        profile,
        password: std::env::var(format!("{var}_PASSWORD")).ok(),
    })
}

fn scratch_name() -> String {
    format!(
        "puricat_it_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

fn spec(value: serde_json::Value) -> NewDatabase {
    serde_json::from_value(value).expect("valid spec")
}

fn change(value: serde_json::Value) -> AlterDatabase {
    serde_json::from_value(value).expect("valid change")
}

fn table_change(value: serde_json::Value) -> TableChange {
    serde_json::from_value(value).expect("valid table change")
}

fn view(table: &TableRef, select: &str, replace: bool) -> ViewSpec {
    ViewSpec {
        table: table.clone(),
        select: select.into(),
        replace,
    }
}

fn at(database: &str, schema: Option<&str>, name: &str) -> TableRef {
    TableRef {
        database: database.into(),
        schema: schema.map(Into::into),
        name: name.into(),
    }
}

async fn objects(session: &Session, database: &str, schema: Option<&str>) -> Vec<String> {
    session
        .list_objects(database, schema)
        .await
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect()
}

async fn has_database(session: &Session, name: &str) -> bool {
    session
        .list_databases()
        .await
        .unwrap()
        .iter()
        .any(|d| d.name == name)
}

#[tokio::test]
async fn mysql_create_database() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    // A plain name, and one that needs quoting to be one identifier.
    let plain = scratch_name();
    let odd = format!("{} ฐาน `ใหม่`", scratch_name());
    let (a, b) = (plain.clone(), odd.clone());

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();

        // The lists the form is filled from.
        let options = session.database_options().await.unwrap();
        let utf8 = options
            .charsets
            .iter()
            .find(|c| c.name == "utf8mb4")
            .expect("utf8mb4");
        assert!(utf8.default_collation.starts_with("utf8mb4_") && !utf8.description.is_empty());
        assert!(options
            .collations
            .iter()
            .any(|c| c.name == "utf8mb4_bin" && c.charset == "utf8mb4"));
        assert!(options.default_charset.is_some() && options.default_collation.is_some());
        assert!(options.owners.is_empty() && options.encodings.is_empty());

        // The statement shown is the statement run; the database exists afterwards.
        let with_defaults = spec(json!({ "name": a }));
        let planned = session.create_database_sql(&with_defaults).unwrap();
        assert!(
            !has_database(&session, &a).await,
            "planning creates nothing"
        );
        assert_eq!(
            session.create_database(&with_defaults).await.unwrap(),
            planned
        );
        assert!(has_database(&session, &a).await);

        let chosen = spec(json!({ "name": b, "charset": "utf8mb4", "collation": "utf8mb4_bin" }));
        session.create_database(&chosen).await.unwrap();
        let row = sqlx::query(
            "SELECT CAST(DEFAULT_CHARACTER_SET_NAME AS CHAR), CAST(DEFAULT_COLLATION_NAME AS CHAR)
             FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
        )
        .bind(&b)
        .fetch_one(&mut admin)
        .await
        .unwrap();
        let text = |i: usize| String::from_utf8(row.get::<Vec<u8>, _>(i)).unwrap();
        assert_eq!(
            (text(0).as_str(), text(1).as_str()),
            ("utf8mb4", "utf8mb4_bin")
        );

        // The server's refusals come back as its own message; nothing is left behind.
        match session.create_database(&with_defaults).await {
            Err(Error::Database(message)) => assert!(message.contains("exists"), "{message}"),
            other => panic!(
                "expected the server to refuse a second database of that name, got {other:?}"
            ),
        }
        let ghost = format!("{a}_ghost");
        let unknown = spec(json!({ "name": ghost, "charset": "no_such_charset" }));
        assert!(matches!(
            session.create_database(&unknown).await,
            Err(Error::Database(_))
        ));
        assert!(!has_database(&session, &ghost).await);
        session.close().await;

        // A read-only session cannot create one.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        assert!(matches!(
            guarded
                .create_database(&spec(json!({ "name": ghost })))
                .await,
            Err(Error::Database(_))
        ));
        assert!(!has_database(&guarded, &ghost).await);
        guarded.close().await;
    })
    .await;

    for name in [&plain, &odd, &format!("{plain}_ghost")] {
        let quoted = name.replace('`', "``");
        admin
            .execute(format!("DROP DATABASE IF EXISTS `{quoted}`").as_str())
            .await
            .unwrap();
    }
    outcome.expect("test body");
}

#[tokio::test]
async fn postgres_create_database() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username)
        .database("postgres");
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
    let plain = scratch_name();
    let odd = format!("{} ฐาน \"ใหม่\"", scratch_name());
    let (a, b) = (plain.clone(), odd.clone());

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref()).await.unwrap();

        let options = session.database_options().await.unwrap();
        let me = options.current_user.clone().expect("current user");
        assert_eq!(me, t.profile.username);
        assert!(options.owners.contains(&me));
        assert!(options.owners.iter().all(|r| !r.starts_with("pg_")));
        assert!(options.templates.contains(&"template0".to_string()));
        assert!(options.templates.contains(&"template1".to_string()));
        assert!(options.encodings.contains(&"UTF8".to_string()));
        assert!(!options.encodings.iter().any(|e| e == "SJIS" || e == "BIG5"), "client-only encodings");
        assert!(options.default_encoding.is_some());
        assert!(options.charsets.is_empty() && options.collations.is_empty());

        let with_defaults = spec(json!({ "name": a }));
        let planned = session.create_database_sql(&with_defaults).unwrap();
        assert!(!has_database(&session, &a).await, "planning creates nothing");
        assert_eq!(session.create_database(&with_defaults).await.unwrap(), planned);
        assert!(has_database(&session, &a).await);

        let chosen = spec(json!({ "name": b, "owner": me, "template": "template0", "encoding": "LATIN1" }));
        // LATIN1 needs a matching locale: C is always available with template0.
        let refused = session.create_database(&chosen).await;
        if let Err(Error::Database(message)) = &refused {
            // A UTF-8 locale server refuses LATIN1: the server's own explanation must reach us.
            assert!(message.to_lowercase().contains("encoding"), "{message}");
            let utf8 = spec(json!({ "name": b, "owner": me, "template": "template0", "encoding": "UTF8" }));
            session.create_database(&utf8).await.unwrap();
        } else {
            refused.unwrap();
        }
        let row = sqlx::query(
            "SELECT pg_get_userbyid(datdba)::text, pg_encoding_to_char(encoding)::text FROM pg_database WHERE datname = $1",
        )
        .bind(&b)
        .fetch_one(&mut admin)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>(0), me);
        assert!(["UTF8", "LATIN1"].contains(&row.get::<String, _>(1).as_str()));

        match session.create_database(&with_defaults).await {
            Err(Error::Database(message)) => assert!(message.contains("already exists"), "{message}"),
            other => panic!("expected the server to refuse a second database of that name, got {other:?}"),
        }
        let ghost = format!("{a}_ghost");
        let no_owner = spec(json!({ "name": ghost, "owner": "puricat_no_such_role" }));
        assert!(matches!(session.create_database(&no_owner).await, Err(Error::Database(_))));
        assert!(!has_database(&session, &ghost).await);
        session.close().await;

        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref()).await.unwrap();
        assert!(matches!(
            guarded.create_database(&spec(json!({ "name": ghost }))).await,
            Err(Error::Database(_))
        ));
        assert!(!has_database(&guarded, &ghost).await);
        guarded.close().await;
    })
    .await;

    for name in [&plain, &odd, &format!("{plain}_ghost")] {
        let quoted = name.replace('"', "\"\"");
        admin
            .execute(format!("DROP DATABASE IF EXISTS \"{quoted}\" WITH (FORCE)").as_str())
            .await
            .unwrap();
    }
    outcome.expect("test body");
}

/// Details, `ALTER DATABASE`, `DROP DATABASE` and `FLUSH` on MySQL.
#[tokio::test]
async fn mysql_alter_and_drop_database() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let plain = scratch_name();
    let odd = format!("{} ฐาน `เก่า`", scratch_name());
    let (a, b) = (plain.clone(), odd.clone());

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        for name in [&a, &b] {
            let made =
                spec(json!({ "name": name, "charset": "utf8mb4", "collation": "utf8mb4_bin" }));
            session.create_database(&made).await.unwrap();
        }
        admin
            .execute(format!("CREATE TABLE `{a}`.t (id INT PRIMARY KEY)").as_str())
            .await
            .unwrap();
        admin
            .execute(format!("CREATE VIEW `{a}`.v AS SELECT id FROM `{a}`.t").as_str())
            .await
            .unwrap();

        // What the forms are filled from.
        let details = session.database_details(&a).await.unwrap();
        assert_eq!(details.name, a);
        assert_eq!(details.charset.as_deref(), Some("utf8mb4"));
        assert_eq!(details.collation.as_deref(), Some("utf8mb4_bin"));
        assert_eq!((details.owner, details.encoding), (None, None));
        assert_eq!(details.tables, Some(2), "a table and a view");
        assert!(!details.protected);
        assert_eq!(session.database_details(&b).await.unwrap().tables, Some(0));
        assert!(session.database_details("mysql").await.unwrap().protected);
        assert!(matches!(
            session.database_details(&format!("{a}_ghost")).await,
            Err(Error::NotFound(_))
        ));

        // The statement shown is the statement run.
        let to_latin1 = change(json!({ "name": a, "charset": "latin1" }));
        let planned = session.alter_database_sql(&to_latin1).unwrap();
        assert_eq!(
            planned,
            vec![format!("ALTER DATABASE `{a}` CHARACTER SET latin1")]
        );
        let unchanged = session.database_details(&a).await.unwrap();
        assert_eq!(
            unchanged.charset.as_deref(),
            Some("utf8mb4"),
            "planning changes nothing"
        );
        assert_eq!(session.alter_database(&to_latin1).await.unwrap(), planned);
        let changed = session.database_details(&a).await.unwrap();
        assert_eq!(changed.charset.as_deref(), Some("latin1"));
        assert_eq!(
            changed.collation.as_deref(),
            Some("latin1_swedish_ci"),
            "the character set's default"
        );

        // A collation alone brings its character set; a quoted name stays one identifier.
        let to_unicode = change(json!({ "name": b, "collation": "utf8mb4_unicode_ci" }));
        session.alter_database(&to_unicode).await.unwrap();
        let changed = session.database_details(&b).await.unwrap();
        assert_eq!(changed.collation.as_deref(), Some("utf8mb4_unicode_ci"));

        // Refused before anything is sent: nothing to change, a rename, the server's own.
        for refused in [
            change(json!({ "name": a })),
            change(json!({ "name": a, "newName": format!("{a}_renamed") })),
            change(json!({ "name": "mysql", "charset": "latin1" })),
        ] {
            assert!(
                matches!(
                    session.alter_database(&refused).await,
                    Err(Error::Invalid(_))
                ),
                "{refused:?}"
            );
        }
        // Refused by the server, in its own words.
        let unknown = change(json!({ "name": a, "charset": "no_such_charset" }));
        assert!(matches!(
            session.alter_database(&unknown).await,
            Err(Error::Database(_))
        ));

        // FLUSH: fixed statements. (`FLUSH LOGS` would start a new binary log file on the
        // developer's server at every run; it was accepted by hand on MySQL 9.3.0.)
        for (what, sql) in [
            (Flush::Privileges, "FLUSH PRIVILEGES"),
            (Flush::Tables, "FLUSH TABLES"),
            (Flush::Status, "FLUSH STATUS"),
        ] {
            assert_eq!(session.flush(what).await.unwrap(), sql);
        }

        // A read-only session can neither change nor drop.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        let back = change(json!({ "name": a, "charset": "utf8mb4" }));
        assert!(matches!(
            guarded.alter_database(&back).await,
            Err(Error::Database(_))
        ));
        assert!(matches!(
            guarded.drop_database(&a).await,
            Err(Error::Database(_))
        ));
        assert!(has_database(&guarded, &a).await);
        guarded.close().await;

        // Dropping: planned first, then gone with everything in it.
        assert!(matches!(
            session.drop_database("mysql").await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            session.drop_database("").await,
            Err(Error::Invalid(_))
        ));
        let planned = session.drop_database_sql(&a).unwrap();
        assert_eq!(planned, format!("DROP DATABASE `{a}`"));
        assert!(has_database(&session, &a).await, "planning drops nothing");
        assert_eq!(session.drop_database(&a).await.unwrap(), planned);
        assert!(!has_database(&session, &a).await);
        assert!(
            matches!(session.drop_database(&a).await, Err(Error::Database(_))),
            "already gone"
        );
        session.drop_database(&b).await.unwrap();
        assert!(!has_database(&session, &b).await);
        assert!(has_database(&session, "mysql").await);
        session.close().await;
    })
    .await;

    for name in [&plain, &odd] {
        let quoted = name.replace('`', "``");
        admin
            .execute(format!("DROP DATABASE IF EXISTS `{quoted}`").as_str())
            .await
            .unwrap();
    }
    outcome.expect("test body");
}

/// Details, `ALTER DATABASE` (owner, rename) and `DROP DATABASE` on PostgreSQL.
#[tokio::test]
async fn postgres_alter_and_drop_database() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username)
        .database("postgres");
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
    let old = scratch_name();
    let new = format!("{} ฐาน \"ใหม่\"", scratch_name());
    let role = format!("{}_role", scratch_name());
    let (a, b, r) = (old.clone(), new.clone(), role.clone());

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        let me = t.profile.username.clone();
        admin
            .execute(format!("CREATE ROLE \"{r}\"").as_str())
            .await
            .unwrap();
        session
            .create_database(&spec(json!({ "name": a })))
            .await
            .unwrap();
        {
            // A table and a view inside it, made by a connection that is gone again
            // before anything is renamed or dropped.
            let mut inside = sqlx::PgConnection::connect_with(&opts.clone().database(&a))
                .await
                .unwrap();
            inside
                .execute("CREATE TABLE t (id int PRIMARY KEY); CREATE VIEW v AS SELECT id FROM t")
                .await
                .unwrap();
            inside.close().await.unwrap();
        }

        let details = session.database_details(&a).await.unwrap();
        assert_eq!(details.owner.as_deref(), Some(me.as_str()));
        assert!(details.encoding.is_some());
        assert_eq!((details.charset, details.collation), (None, None));
        assert_eq!(
            details.tables,
            Some(2),
            "a table and a view; the catalogue is not counted"
        );
        assert!(!details.protected);
        // A database nobody can connect to is still described, without the number.
        let template0 = session.database_details("template0").await.unwrap();
        assert!(template0.protected && template0.owner.is_some());
        assert_eq!(template0.tables, None);
        assert!(matches!(
            session.database_details(&format!("{a}_ghost")).await,
            Err(Error::NotFound(_))
        ));

        // Owner first, then the new name; the statements shown are the statements run. The
        // session's own connections to the database (opened for the details) do not stand
        // in the way.
        let both = change(json!({ "name": a, "owner": r, "newName": b }));
        let planned = session.alter_database_sql(&both).unwrap();
        let quoted = b.replace('"', "\"\"");
        assert_eq!(
            planned,
            vec![
                format!("ALTER DATABASE \"{a}\" OWNER TO \"{r}\""),
                format!("ALTER DATABASE \"{a}\" RENAME TO \"{quoted}\""),
            ]
        );
        assert!(has_database(&session, &a).await, "planning changes nothing");
        assert_eq!(session.alter_database(&both).await.unwrap(), planned);
        assert!(!has_database(&session, &a).await && has_database(&session, &b).await);
        let owner_of = |name: String| {
            sqlx::query("SELECT pg_get_userbyid(datdba)::text FROM pg_database WHERE datname = $1")
                .bind(name)
        };
        let owner: String = owner_of(b.clone())
            .fetch_one(&mut admin)
            .await
            .unwrap()
            .get(0);
        assert_eq!(owner, r);

        // All or nothing: the owner is not changed when the rename after it fails.
        let half = change(json!({ "name": b, "owner": me, "newName": "template1" }));
        match session.alter_database(&half).await {
            Err(Error::Database(message)) => {
                assert!(message.contains("already exists"), "{message}")
            }
            other => panic!("expected the server to refuse the name, got {other:?}"),
        }
        let owner: String = owner_of(b.clone())
            .fetch_one(&mut admin)
            .await
            .unwrap()
            .get(0);
        assert_eq!(owner, r, "the owner change was rolled back with the rename");

        // Refused before anything is sent.
        for refused in [
            change(json!({ "name": b })),
            change(json!({ "name": b, "newName": b })),
            change(json!({ "name": b, "charset": "utf8mb4" })),
            change(json!({ "name": "postgres", "owner": r })),
        ] {
            assert!(
                matches!(
                    session.alter_database(&refused).await,
                    Err(Error::Invalid(_))
                ),
                "{refused:?}"
            );
        }
        for protected in ["postgres", "template0", "template1"] {
            assert!(
                matches!(
                    session.drop_database(protected).await,
                    Err(Error::Invalid(_))
                ),
                "{protected}"
            );
        }
        assert!(matches!(
            session.flush(Flush::Tables).await,
            Err(Error::Unsupported(_))
        ));

        // A session opened on the database itself can change its owner, not drop or rename it.
        let mut on_it = t.profile.clone();
        on_it.database = Some(b.clone());
        let own = Session::connect(&on_it, t.password.as_deref())
            .await
            .unwrap();
        match own.drop_database_sql(&b) {
            Err(Error::Invalid(message)) => assert!(message.contains("opened on"), "{message}"),
            other => panic!("expected a refusal for the session's own database, got {other:?}"),
        }
        assert!(matches!(
            own.alter_database_sql(&change(json!({ "name": b, "newName": format!("{a}_x") }))),
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            own.alter_database_sql(&change(json!({ "name": b, "owner": me })))
                .unwrap()
                .len(),
            1
        );
        own.close().await;

        // A read-only session can neither change nor drop.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        assert!(matches!(
            guarded
                .alter_database(&change(json!({ "name": b, "owner": me })))
                .await,
            Err(Error::Database(_))
        ));
        assert!(matches!(
            guarded.drop_database(&b).await,
            Err(Error::Database(_))
        ));
        assert!(has_database(&guarded, &b).await);
        guarded.close().await;

        // Dropping: planned first, then gone — although this session looked into it a moment ago.
        assert_eq!(session.database_details(&b).await.unwrap().tables, Some(2));
        let planned = session.drop_database_sql(&b).unwrap();
        assert_eq!(planned, format!("DROP DATABASE \"{quoted}\""));
        assert!(has_database(&session, &b).await, "planning drops nothing");
        assert_eq!(session.drop_database(&b).await.unwrap(), planned);
        assert!(!has_database(&session, &b).await);
        assert!(
            matches!(session.drop_database(&b).await, Err(Error::Database(_))),
            "already gone"
        );
        session.close().await;
    })
    .await;

    for name in [&old, &new] {
        let quoted = name.replace('"', "\"\"");
        admin
            .execute(format!("DROP DATABASE IF EXISTS \"{quoted}\" WITH (FORCE)").as_str())
            .await
            .unwrap();
    }
    admin
        .execute(format!("DROP ROLE IF EXISTS \"{role}\"").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

/// The table menu on MySQL: the server's own definition, then rename, empty and drop.
#[tokio::test]
async fn mysql_table_menu() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let database = scratch_name();
    let db = database.clone();

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        // A name that needs quoting to be one identifier.
        let odd = "ใบสั่ง `ซื้อ`";
        let quoted = odd.replace('`', "``");
        for sql in [
            format!("CREATE DATABASE `{db}`"),
            format!(
                "CREATE TABLE `{db}`.`{quoted}` (id INT PRIMARY KEY AUTO_INCREMENT,
                 name VARCHAR(20) NOT NULL DEFAULT 'x', KEY by_name (name)) COMMENT 'จดไว้'"
            ),
            format!("INSERT INTO `{db}`.`{quoted}` (name) VALUES ('a'), ('b')"),
            format!("CREATE VIEW `{db}`.v AS SELECT id FROM `{db}`.`{quoted}`"),
        ] {
            admin.execute(sql.as_str()).await.unwrap();
        }
        let table = at(&db, None, odd);

        // The definition is the server's own text.
        let definition = session
            .table_definition(&table, ObjectKind::Table)
            .await
            .unwrap();
        assert_eq!(definition.source, DefinitionSource::Server);
        assert!(
            definition
                .sql
                .starts_with(&format!("CREATE TABLE `{quoted}` (")),
            "{}",
            definition.sql
        );
        for piece in ["AUTO_INCREMENT", "KEY `by_name` (`name`)", "COMMENT='จดไว้'"] {
            assert!(
                definition.sql.contains(piece),
                "{piece}: {}",
                definition.sql
            );
        }
        let view = session
            .table_definition(&at(&db, None, "v"), ObjectKind::View)
            .await
            .unwrap();
        assert!(
            view.sql.starts_with("CREATE ")
                && view.sql.contains(&format!(" VIEW `{db}`.`v` AS select ")),
            "{}",
            view.sql
        );
        assert!(session
            .table_definition(&at(&db, None, "ghost"), ObjectKind::Table)
            .await
            .is_err());

        // Rename: planned first, and the statement shown is the statement run.
        let rename = table_change(json!({
            "action": "rename", "table": table, "kind": "table", "newName": " orders ",
        }));
        let planned = session.table_change_sql(&rename).unwrap();
        assert_eq!(
            planned,
            format!("RENAME TABLE `{db}`.`{quoted}` TO `{db}`.`orders`")
        );
        assert!(objects(&session, &db, None)
            .await
            .contains(&odd.to_string()));
        assert_eq!(session.table_change(&rename).await.unwrap(), planned);
        let view_renamed = table_change(json!({
            "action": "rename", "table": at(&db, None, "v"), "kind": "view", "newName": "v2",
        }));
        session.table_change(&view_renamed).await.unwrap();
        assert_eq!(objects(&session, &db, None).await, ["orders", "v2"]);
        // A name that is taken: the server's refusal, nothing changed.
        let taken = table_change(json!({
            "action": "rename", "table": at(&db, None, "orders"), "kind": "table", "newName": "v2",
        }));
        assert!(matches!(
            session.table_change(&taken).await,
            Err(Error::Database(_))
        ));

        // Empty: every row goes and AUTO_INCREMENT starts again.
        let orders = at(&db, None, "orders");
        let empty = table_change(json!({ "action": "empty", "table": orders }));
        assert_eq!(
            session.table_change(&empty).await.unwrap(),
            format!("TRUNCATE TABLE `{db}`.`orders`")
        );
        admin
            .execute(format!("INSERT INTO `{db}`.orders (name) VALUES ('c')").as_str())
            .await
            .unwrap();
        let ids: Vec<i32> = sqlx::query(format!("SELECT id FROM `{db}`.orders").as_str())
            .fetch_all(&mut admin)
            .await
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect();
        assert_eq!(ids, [1]);

        // A read-only session is refused by the server.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        assert!(matches!(
            guarded.table_change(&empty).await,
            Err(Error::Database(_))
        ));
        guarded.close().await;

        // Drop: the view, then the table.
        let drop_view = table_change(json!({
            "action": "drop", "table": at(&db, None, "v2"), "kind": "view",
        }));
        assert_eq!(
            session.table_change(&drop_view).await.unwrap(),
            format!("DROP VIEW `{db}`.`v2`")
        );
        let drop_table =
            table_change(json!({ "action": "drop", "table": orders, "kind": "table" }));
        let planned = session.table_change_sql(&drop_table).unwrap();
        assert_eq!(planned, format!("DROP TABLE `{db}`.`orders`"));
        assert_eq!(objects(&session, &db, None).await, ["orders"]);
        assert_eq!(session.table_change(&drop_table).await.unwrap(), planned);
        assert!(objects(&session, &db, None).await.is_empty());
        assert!(
            matches!(
                session.table_change(&drop_table).await,
                Err(Error::Database(_))
            ),
            "already gone"
        );
        session.close().await;
    })
    .await;

    admin
        .execute(format!("DROP DATABASE IF EXISTS `{database}`").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

/// The table menu on PostgreSQL. The definition is put together from the catalogue, so it
/// is checked the hard way: run in a second database, it must make a table whose own
/// definition is the same text.
#[tokio::test]
async fn postgres_table_menu() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username)
        .database("postgres");
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
    let (first, second) = (scratch_name(), scratch_name());
    let (a, b) = (first.clone(), second.clone());

    // What the sample table leans on, needed in both databases.
    const AROUND: &str = r#"
        CREATE SCHEMA shop;
        CREATE TABLE shop.customers (
            id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, email text NOT NULL UNIQUE);
        CREATE FUNCTION shop.touch() RETURNS trigger LANGUAGE plpgsql AS 'BEGIN RETURN NEW; END';
    "#;
    const SAMPLE: &str = r#"
        CREATE TABLE shop."order lines" (
            id bigint GENERATED BY DEFAULT AS IDENTITY,
            customer_id int NOT NULL REFERENCES shop.customers (id) ON DELETE CASCADE,
            qty int NOT NULL DEFAULT 1 CHECK (qty > 0),
            price numeric(12,2) NOT NULL DEFAULT 0,
            total numeric(14,2) GENERATED ALWAYS AS (qty * price) STORED,
            note varchar(40) COLLATE "C",
            made timestamptz NOT NULL DEFAULT now(),
            PRIMARY KEY (id)
        );
        CREATE INDEX order_lines_made ON shop."order lines" (made DESC) WHERE qty > 1;
        CREATE TRIGGER touch BEFORE UPDATE ON shop."order lines"
            FOR EACH ROW EXECUTE FUNCTION shop.touch();
        COMMENT ON TABLE shop."order lines" IS 'บรรทัดของ ''ใบสั่ง''';
        COMMENT ON COLUMN shop."order lines".qty IS 'how many';
        CREATE VIEW shop.big AS SELECT id, qty FROM shop."order lines" WHERE qty > 10;
        CREATE MATERIALIZED VIEW shop.totals AS
            SELECT customer_id, sum(total) AS total FROM shop."order lines" GROUP BY customer_id;
        CREATE INDEX totals_by_customer ON shop.totals (customer_id);
        INSERT INTO shop.customers (email) VALUES ('a@example.com');
        INSERT INTO shop."order lines" (customer_id, qty, price) VALUES (1, 2, 10), (1, 20, 1);
    "#;

    let outcome = tokio::spawn(async move {
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        for name in [&a, &b] {
            session
                .create_database(&spec(json!({ "name": name })))
                .await
                .unwrap();
        }
        let mut inside = sqlx::PgConnection::connect_with(&opts.clone().database(&a))
            .await
            .unwrap();
        inside.execute(AROUND).await.unwrap();
        inside.execute(SAMPLE).await.unwrap();
        let shop = |database: &str, name: &str| at(database, Some("shop"), name);
        let lines = shop(&a, "order lines");

        let definition = session
            .table_definition(&lines, ObjectKind::Table)
            .await
            .unwrap();
        assert_eq!(definition.source, DefinitionSource::Catalogue);
        eprintln!("--- PostgreSQL definition ---\n{}\n---", definition.sql);
        for piece in [
            "CREATE TABLE \"shop\".\"order lines\" (\n    id bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n",
            "    qty integer DEFAULT 1 NOT NULL,",
            "    total numeric(14,2) GENERATED ALWAYS AS (",
            "    note character varying(40) COLLATE pg_catalog.\"C\",",
            "    CONSTRAINT \"order lines_pkey\" PRIMARY KEY (id),",
            "    CONSTRAINT \"order lines_qty_check\" CHECK (qty > 0),",
            "    CONSTRAINT \"order lines_customer_id_fkey\" FOREIGN KEY (customer_id) REFERENCES shop.customers(id) ON DELETE CASCADE\n);",
            "CREATE INDEX order_lines_made ON shop.\"order lines\" USING btree (made DESC) WHERE (qty > 1);",
            "CREATE TRIGGER touch BEFORE UPDATE ON shop.\"order lines\" FOR EACH ROW EXECUTE FUNCTION shop.touch();",
            "COMMENT ON TABLE \"shop\".\"order lines\" IS 'บรรทัดของ ''ใบสั่ง''';",
            "COMMENT ON COLUMN \"shop\".\"order lines\".qty IS 'how many';",
        ] {
            assert!(definition.sql.contains(piece), "{piece}\n{}", definition.sql);
        }
        assert!(definition.sql.starts_with("CREATE TABLE "));
        // The index a constraint makes by itself is not listed a second time.
        assert!(!definition.sql.contains("CREATE UNIQUE INDEX"));

        // Run in the second database, the text makes the same table.
        {
            let mut other = sqlx::PgConnection::connect_with(&opts.clone().database(&b))
                .await
                .unwrap();
            other.execute(AROUND).await.unwrap();
            other.execute(definition.sql.as_str()).await.unwrap();
            other.close().await.unwrap();
        }
        let copy = shop(&b, "order lines");
        assert_eq!(
            session
                .table_definition(&copy, ObjectKind::Table)
                .await
                .unwrap(),
            definition
        );
        let columns = |meta: puricat_core::db::TableMeta| {
            (
                meta.primary_key,
                meta.columns
                    .into_iter()
                    .map(|c| (c.name, c.data_type, c.nullable, c.default, c.generated))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            columns(session.describe_table(&copy).await.unwrap()),
            columns(session.describe_table(&lines).await.unwrap())
        );

        // Views: the stored query under a CREATE line; a materialized view with its index.
        let big = session
            .table_definition(&shop(&a, "big"), ObjectKind::View)
            .await
            .unwrap();
        assert!(
            big.sql.starts_with("CREATE VIEW \"shop\".\"big\" AS\n SELECT")
                && big.sql.ends_with(';'),
            "{}",
            big.sql
        );
        let totals = session
            .table_definition(&shop(&a, "totals"), ObjectKind::Matview)
            .await
            .unwrap();
        assert!(
            totals
                .sql
                .starts_with("CREATE MATERIALIZED VIEW \"shop\".\"totals\" AS\n SELECT")
                && totals.sql.ends_with(
                    "CREATE INDEX totals_by_customer ON shop.totals USING btree (customer_id);"
                ),
            "{}",
            totals.sql
        );
        // A table is not found as a view, nor one that does not exist.
        for (name, kind) in [
            ("order lines", ObjectKind::View),
            ("ghost", ObjectKind::Table),
        ] {
            assert!(matches!(
                session.table_definition(&shop(&a, name), kind).await,
                Err(Error::NotFound(_))
            ));
        }

        // Rename: planned, then run inside the database that holds the table (the session
        // itself is opened on another one).
        let rename = table_change(json!({
            "action": "rename", "table": lines, "kind": "table", "newName": "lines \"2026\"",
        }));
        let planned = session.table_change_sql(&rename).unwrap();
        assert_eq!(
            planned,
            "ALTER TABLE \"shop\".\"order lines\" RENAME TO \"lines \"\"2026\"\"\""
        );
        assert!(objects(&session, &a, Some("shop"))
            .await
            .contains(&"order lines".to_string()));
        assert_eq!(session.table_change(&rename).await.unwrap(), planned);
        for (name, kind, word) in [
            ("big", "view", "VIEW"),
            ("totals", "matview", "MATERIALIZED VIEW"),
        ] {
            let change = table_change(json!({
                "action": "rename", "table": shop(&a, name), "kind": kind,
                "newName": format!("{name}2"),
            }));
            assert_eq!(
                session.table_change(&change).await.unwrap(),
                format!("ALTER {word} \"shop\".\"{name}\" RENAME TO \"{name}2\"")
            );
        }
        let mut names = objects(&session, &a, Some("shop")).await;
        names.sort();
        assert_eq!(names, ["big2", "customers", "lines \"2026\"", "totals2"]);
        // The copy in the other database was not touched.
        assert!(objects(&session, &b, Some("shop"))
            .await
            .contains(&"order lines".to_string()));

        // Empty: a table another one points at is refused by the server and keeps its rows.
        let customers = table_change(json!({ "action": "empty", "table": shop(&a, "customers") }));
        assert!(matches!(
            session.table_change(&customers).await,
            Err(Error::Database(_))
        ));
        let renamed = shop(&a, "lines \"2026\"");
        let empty = table_change(json!({ "action": "empty", "table": renamed }));
        assert_eq!(
            session.table_change(&empty).await.unwrap(),
            "TRUNCATE TABLE \"shop\".\"lines \"\"2026\"\"\""
        );
        let left: Vec<i64> = sqlx::query(
            "SELECT count(*) FROM shop.customers
             UNION ALL SELECT count(*) FROM shop.\"lines \"\"2026\"\"\"",
        )
        .fetch_all(&mut inside)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
        assert_eq!(left, [1, 0]);

        // A read-only session is refused by the server.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        assert!(matches!(
            guarded.table_change(&empty).await,
            Err(Error::Database(_))
        ));
        guarded.close().await;

        // Drop: no CASCADE, so a table that views depend on is refused until they are gone.
        let drop_table =
            table_change(json!({ "action": "drop", "table": renamed, "kind": "table" }));
        assert!(matches!(
            session.table_change(&drop_table).await,
            Err(Error::Database(_))
        ));
        for (name, kind, word) in [
            ("big2", "view", "VIEW"),
            ("totals2", "matview", "MATERIALIZED VIEW"),
        ] {
            let change = table_change(json!({
                "action": "drop", "table": shop(&a, name), "kind": kind,
            }));
            assert_eq!(
                session.table_change(&change).await.unwrap(),
                format!("DROP {word} \"shop\".\"{name}\"")
            );
        }
        let planned = session.table_change_sql(&drop_table).unwrap();
        assert_eq!(planned, "DROP TABLE \"shop\".\"lines \"\"2026\"\"\"");
        assert_eq!(session.table_change(&drop_table).await.unwrap(), planned);
        assert_eq!(objects(&session, &a, Some("shop")).await, ["customers"]);
        inside.close().await.unwrap();
        session.close().await;
    })
    .await;

    for name in [&first, &second] {
        admin
            .execute(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)").as_str())
            .await
            .unwrap();
    }
    outcome.expect("test body");
}

async fn my_count(admin: &mut sqlx::MySqlConnection, sql: &str) -> i64 {
    sqlx::query(sql).fetch_one(admin).await.unwrap().get(0)
}

/// The view form on MySQL: create, read the query back, replace it, try a query.
#[tokio::test]
async fn mysql_view_form() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let database = scratch_name();
    let db = database.clone();

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        for sql in [
            format!("CREATE DATABASE `{db}`"),
            format!("CREATE TABLE `{db}`.orders (id INT PRIMARY KEY, total DECIMAL(10,2), note VARCHAR(20))"),
            format!("INSERT INTO `{db}`.orders VALUES (1, 50, 'a'), (2, 150, 'b'), (3, 250, 'c')"),
        ] {
            admin.execute(sql.as_str()).await.unwrap();
        }
        let big = at(&db, None, "big orders");

        // The table is named without its database: the view's database is where MySQL
        // must look for it, whatever the session's own default database is.
        let create = view(&big, "SELECT id, total FROM orders WHERE total > 100;", false);
        let planned = session.view_statements_sql(&create).unwrap();
        assert_eq!(
            planned,
            [format!("CREATE VIEW `{db}`.`big orders` AS\nSELECT id, total FROM orders WHERE total > 100")]
        );
        assert_eq!(objects(&session, &db, None).await, ["orders"], "planning runs nothing");
        assert_eq!(session.save_view(&create).await.unwrap(), planned);
        assert_eq!(objects(&session, &db, None).await, ["big orders", "orders"]);
        assert_eq!(
            my_count(&mut admin, &format!("SELECT COUNT(*) FROM `{db}`.`big orders`")).await,
            2
        );

        // The query comes back as the server keeps it: rewritten, every name qualified.
        let kept = session.view_select(&big).await.unwrap();
        assert!(kept.to_lowercase().starts_with("select "), "{kept}");
        assert!(kept.contains(&format!("`{db}`.`orders`")), "{kept}");
        // ... and that text is itself a query the form accepts.
        assert!(session.view_statements_sql(&view(&big, &kept, true)).is_ok());

        // The same name again: the server's refusal, and the view is as it was.
        assert!(matches!(session.save_view(&create).await, Err(Error::Database(_))));

        // Another WHERE and one more column.
        let replace = view(&big, "SELECT id, total, note FROM orders WHERE total > 200", true);
        assert_eq!(
            session.save_view(&replace).await.unwrap(),
            [format!("CREATE OR REPLACE VIEW `{db}`.`big orders` AS\nSELECT id, total, note FROM orders WHERE total > 200")]
        );
        let columns = session.describe_table(&big).await.unwrap().columns;
        assert_eq!(
            columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["id", "total", "note"]
        );
        assert_eq!(
            my_count(&mut admin, &format!("SELECT COUNT(*) FROM `{db}`.`big orders`")).await,
            1
        );

        // A query that fails on the server leaves the view alone.
        assert!(matches!(
            session.save_view(&view(&big, "SELECT nothing FROM nowhere", true)).await,
            Err(Error::Database(_))
        ));
        assert!(session.view_select(&big).await.unwrap().contains("200"));

        // Trying a query: rows, read in the view's database; a statement that writes is
        // refused before anything is sent, and the rows are still there.
        let tried = session
            .preview_select(&db, "SELECT id, note FROM orders ORDER BY id", 2)
            .await
            .unwrap();
        assert_eq!(
            tried.result.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["id", "note"]
        );
        assert_eq!(
            tried.result.rows,
            [
                [Some("1".to_string()), Some("a".to_string())],
                [Some("2".to_string()), Some("b".to_string())]
            ]
        );
        assert!(tried.result.capped, "a third row exists");
        assert!(matches!(
            session.preview_select(&db, "DELETE FROM orders", 10).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            session.preview_select(&db, "SELECT 1; DELETE FROM orders", 10).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            session.preview_select(&db, "SELECT nothing FROM nowhere", 10).await,
            Err(Error::Database(_))
        ));
        assert_eq!(my_count(&mut admin, &format!("SELECT COUNT(*) FROM `{db}`.orders")).await, 3);

        // A view that is not there.
        assert!(matches!(
            session.view_select(&at(&db, None, "missing")).await,
            Err(Error::NotFound(_))
        ));
        session.close().await;
    })
    .await;

    admin
        .execute(format!("DROP DATABASE IF EXISTS `{database}`").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

/// The view form on PostgreSQL, where `OR REPLACE` may add columns but not take one away,
/// and where a `WITH` can hide a statement that writes.
#[tokio::test]
async fn postgres_view_form() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username)
        .database("postgres");
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    let mut admin = sqlx::PgConnection::connect_with(&opts).await.unwrap();
    let database = scratch_name();
    let db = database.clone();

    let outcome = tokio::spawn(async move {
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        session
            .create_database(&spec(json!({ "name": db })))
            .await
            .unwrap();
        let mut inside = sqlx::PgConnection::connect_with(&opts.clone().database(&db))
            .await
            .unwrap();
        inside
            .execute(
                "CREATE SCHEMA shop;
                 CREATE TABLE shop.orders (id int PRIMARY KEY, total numeric(10,2), note text);
                 INSERT INTO shop.orders VALUES (1, 50, 'a'), (2, 150, 'b'), (3, 250, 'c');",
            )
            .await
            .unwrap();
        let big = at(&db, Some("shop"), "big orders");

        // Created inside the database that holds the schema, not the session's own.
        let create = view(&big, "SELECT id, total FROM shop.orders WHERE total > 100", false);
        let planned = session.view_statements_sql(&create).unwrap();
        assert_eq!(
            planned,
            ["CREATE VIEW \"shop\".\"big orders\" AS\nSELECT id, total FROM shop.orders WHERE total > 100"]
        );
        assert_eq!(session.save_view(&create).await.unwrap(), planned);
        assert_eq!(objects(&session, &db, Some("shop")).await, ["big orders", "orders"]);

        // The query as PostgreSQL prints it, without the semicolon it ends with.
        let kept = session.view_select(&big).await.unwrap();
        assert!(kept.trim_start().to_lowercase().starts_with("select"), "{kept}");
        assert!(kept.contains("100") && !kept.trim_end().ends_with(';'), "{kept}");
        assert!(session.view_statements_sql(&view(&big, &kept, true)).is_ok());

        // Another WHERE and a column added at the end.
        let replace = view(&big, "SELECT id, total, note FROM shop.orders WHERE total > 200", true);
        assert_eq!(
            session.save_view(&replace).await.unwrap(),
            ["CREATE OR REPLACE VIEW \"shop\".\"big orders\" AS\nSELECT id, total, note FROM shop.orders WHERE total > 200"]
        );
        let rows: i64 = sqlx::query("SELECT count(*) FROM shop.\"big orders\"")
            .fetch_one(&mut inside)
            .await
            .unwrap()
            .get(0);
        assert_eq!(rows, 1);

        // A column taken away: PostgreSQL refuses, and the view stays as it was.
        let fewer = view(&big, "SELECT id FROM shop.orders", true);
        match session.save_view(&fewer).await {
            Err(Error::Database(message)) => assert!(message.contains("drop columns"), "{message}"),
            other => panic!("expected the server's refusal, got {other:?}"),
        }
        let kept = session.view_select(&big).await.unwrap();
        assert!(kept.contains("note") && kept.contains("200"), "{kept}");

        // Trying a query: rows; a WITH that deletes is refused without touching the rows,
        // by the reading of the text, and the server's read-only transaction stands behind it.
        let tried = session
            .preview_select(&db, "SELECT id, note FROM shop.orders ORDER BY id", 2)
            .await
            .unwrap();
        assert_eq!(
            tried.result.rows,
            [
                [Some("1".to_string()), Some("a".to_string())],
                [Some("2".to_string()), Some("b".to_string())]
            ]
        );
        assert!(tried.result.capped);
        let smuggled = "WITH gone AS (DELETE FROM shop.orders RETURNING id) SELECT id FROM gone";
        assert!(matches!(
            session.preview_select(&db, smuggled, 10).await,
            Err(Error::Invalid(_))
        ));
        assert!(session.view_statements_sql(&view(&big, smuggled, true)).is_err());
        // A function that writes gets past the reading of the text; the transaction stops it.
        inside
            .execute(
                "CREATE FUNCTION shop.wipe() RETURNS int LANGUAGE sql AS
                 'DELETE FROM shop.orders; SELECT 1'",
            )
            .await
            .unwrap();
        assert!(matches!(
            session.preview_select(&db, "SELECT shop.wipe()", 10).await,
            Err(Error::Database(_))
        ));
        let left: i64 = sqlx::query("SELECT count(*) FROM shop.orders")
            .fetch_one(&mut inside)
            .await
            .unwrap()
            .get(0);
        assert_eq!(left, 3);

        // A materialized view is not a view this form edits.
        inside
            .execute("CREATE MATERIALIZED VIEW shop.totals AS SELECT sum(total) AS t FROM shop.orders")
            .await
            .unwrap();
        assert!(matches!(
            session.view_select(&at(&db, Some("shop"), "totals")).await,
            Err(Error::NotFound(_))
        ));

        // A read-only session is refused by the server.
        let mut read_only = t.profile.clone();
        read_only.read_only = true;
        let guarded = Session::connect(&read_only, t.password.as_deref())
            .await
            .unwrap();
        assert!(matches!(guarded.save_view(&replace).await, Err(Error::Database(_))));
        guarded.close().await;
        inside.close().await.unwrap();
        session.close().await;
    })
    .await;

    admin
        .execute(format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)").as_str())
        .await
        .unwrap();
    outcome.expect("test body");
}

/// A SQLite connection is one file: there is nothing to create inside it.
#[tokio::test]
async fn sqlite_has_no_databases_to_create() {
    let path = std::env::temp_dir().join(format!("{}.sqlite", scratch_name()));
    {
        use sqlx::sqlite::SqliteConnectOptions;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        conn.execute("CREATE TABLE t (a)").await.unwrap();
    }
    let profile = serde_json::from_value::<ConnectionProfile>(json!({
        "name": "file", "engine": "sqlite", "file": path.to_string_lossy(), "connectTimeoutSecs": 1,
    }))
    .unwrap()
    .normalized()
    .unwrap();
    let outcome = async {
        let session = Session::connect(&profile, None).await?;
        let options = session.database_options().await;
        let created = session.create_database(&spec(json!({ "name": "x" }))).await;
        // Nor is there one to describe, change or drop, or a server to flush.
        let others = matches!(
            session.database_details("main").await,
            Err(Error::Unsupported(_))
        ) && matches!(
            session.drop_database_sql("main"),
            Err(Error::Unsupported(_))
        ) && matches!(
            session.alter_database_sql(&change(json!({ "name": "main", "newName": "x" }))),
            Err(Error::Unsupported(_))
        ) && matches!(
            session.flush(Flush::Tables).await,
            Err(Error::Unsupported(_))
        );
        session.close().await;
        Ok::<_, Error>((
            options.is_err(),
            matches!(created, Err(Error::Unsupported(_))),
            others,
        ))
    }
    .await;
    let _ = std::fs::remove_file(&path);
    assert_eq!(outcome.unwrap(), (true, true, true));
}
