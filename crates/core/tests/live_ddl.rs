//! Integration tests for schema changes made through a form, against real servers.
//! Same opt-in environment variables and disposable-database rule as `live_db.rs`: every
//! database created here is named `puricat_it_<random>…` and dropped again.

use puricat_core::db::ddl::NewDatabase;
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
        session.close().await;
        Ok::<_, Error>((
            options.is_err(),
            matches!(created, Err(Error::Unsupported(_))),
        ))
    }
    .await;
    let _ = std::fs::remove_file(&path);
    assert_eq!(outcome.unwrap(), (true, true));
}
