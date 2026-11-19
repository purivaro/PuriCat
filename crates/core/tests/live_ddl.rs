//! Integration tests for schema changes made through a form, against real servers.
//! Same opt-in environment variables and disposable-database rule as `live_db.rs`: every
//! database created here is named `puricat_it_<random>…` and dropped again.

use puricat_core::db::ddl::{AlterDatabase, Flush, NewDatabase};
use puricat_core::db::Session;
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
