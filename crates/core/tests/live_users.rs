//! Integration tests for the user forms, against real servers. Same opt-in environment
//! variables as `live_db.rs`. Every account, role and database made here is named
//! `puricat_it_<random>…`, and whatever a test made is dropped again, also when it fails.
//!
//! What "the password is right" means differs per server. The local MySQL asks for the
//! password, so the tests log in with it. A PostgreSQL that trusts local connections (the
//! Homebrew default) asks for none: there the stored verifier is compared with an md5 the
//! server computes from the same password sent as a bind parameter — another path than the
//! literal under test — on a connection that stores passwords as md5.

use puricat_core::db::users::{self, AccountRef, Facts, Show, Step, UserSpec, MASK};
use puricat_core::db::Session;
use puricat_core::model::{ConnectionProfile, Engine};
use puricat_core::Error;
use serde_json::json;
use sqlx::{Connection, Executor, Row};

/// Quotes of every kind, a backslash, a statement separator, a comment, Thai, and a space
/// at each end (a password is never trimmed).
const HOSTILE: &str = " p'w\"d \\ ผ่าน; -- `x` ";
const SECOND: &str = "n3w \\\\ 'pw' ใหม่";

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

/// A prefix of this test's own, so cleaning up by prefix never meets another test's work.
fn scratch_prefix() -> String {
    format!(
        "puricat_it_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

fn user(
    name: &str,
    host: Option<&str>,
    template: &str,
    database: Option<&str>,
    password: &str,
) -> UserSpec {
    serde_json::from_value(json!({
        "account": { "name": name, "host": host },
        "template": template, "database": database, "password": password,
    }))
    .expect("valid spec")
}

fn at(name: &str, host: Option<&str>) -> AccountRef {
    AccountRef {
        name: name.into(),
        host: host.map(str::to_string),
    }
}

fn texts(steps: &[Step]) -> Vec<&str> {
    steps.iter().map(|s| s.sql.as_str()).collect()
}

// ---- MySQL ---------------------------------------------------------------------------

fn mysql_options(t: &Target) -> sqlx::mysql::MySqlConnectOptions {
    let mut opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(&t.profile.username);
    if let Some(pw) = &t.password {
        opts = opts.password(pw);
    }
    opts
}

/// Log in as an account the test made.
async fn mysql_as(
    t: &Target,
    name: &str,
    password: &str,
) -> std::result::Result<sqlx::MySqlConnection, sqlx::Error> {
    let opts = sqlx::mysql::MySqlConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(name)
        .password(password);
    sqlx::MySqlConnection::connect_with(&opts).await
}

async fn mysql_count(
    conn: &mut sqlx::MySqlConnection,
    table: &str,
) -> std::result::Result<i64, sqlx::Error> {
    let row = sqlx::query(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(conn)
        .await?;
    row.try_get(0)
}

async fn mysql_accounts(admin: &mut sqlx::MySqlConnection, prefix: &str) -> Vec<String> {
    sqlx::query("SELECT CAST(User AS CHAR) FROM mysql.user WHERE LEFT(User, ?) = ? ORDER BY 1")
        .bind(prefix.len() as i64)
        .bind(prefix)
        .fetch_all(admin)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>(0))
        .collect()
}

#[tokio::test]
async fn mysql_users() {
    let Some(t) = target(Engine::Mysql, "PURICAT_TEST_MYSQL") else {
        eprintln!("skipped: PURICAT_TEST_MYSQL is not set");
        return;
    };
    let opts = mysql_options(&t);
    let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
    let p = scratch_prefix();
    // Two databases, and a third whose name the first one's would match as a pattern.
    let (a, b, wild) = (format!("{p}_a"), format!("{p}_b"), format!("{p}Xa"));
    let (prefix, dbs) = (p.clone(), [a.clone(), b.clone(), wild.clone()]);

    let outcome = tokio::spawn(async move {
        let mut admin = sqlx::MySqlConnection::connect_with(&opts).await.unwrap();
        for db in [&a, &b, &wild] {
            for sql in [
                format!("CREATE DATABASE `{db}`"),
                format!("CREATE TABLE `{db}`.t (id INT PRIMARY KEY, note VARCHAR(20))"),
                format!("INSERT INTO `{db}`.t VALUES (1, 'one'), (2, 'two')"),
                format!("CREATE VIEW `{db}`.v AS SELECT id FROM `{db}`.t"),
            ] {
                admin.execute(sql.as_str()).await.unwrap();
            }
        }
        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        let (app, reader, bare) = (
            format!("{p}_app"),
            format!("{p}_reader"),
            format!("{p}_bare"),
        );
        // `_` is a wildcard in the database of a grant on this server unless escaped.
        let escaped = a.replace('_', "\\_");

        // The list as it is: this connection's own account, and the server's own.
        let before = session.list_accounts().await.unwrap();
        let me = before.iter().find(|x| x.you).expect("own account");
        assert_eq!(me.name, t.profile.username);
        assert!(!me.system && !me.locked);
        let sys = before
            .iter()
            .find(|x| x.name == "mysql.sys")
            .expect("mysql.sys");
        assert!(sys.system && sys.locked && !sys.you);

        // Application user: the preview, masked and revealed, then the account.
        let spec = user(&app, None, "app", Some(&a), HOSTILE);
        let masked = session.plan_user(&spec, false).await.unwrap();
        assert_eq!(
            texts(&masked),
            [
                format!("CREATE USER '{app}'@'%' IDENTIFIED BY {MASK}"),
                format!("GRANT ALL PRIVILEGES ON `{escaped}`.* TO '{app}'@'%'"),
            ]
        );
        let revealed = session.plan_user(&spec, true).await.unwrap();
        assert_eq!(
            revealed[0].sql,
            format!("CREATE USER '{app}'@'%' IDENTIFIED BY ' p''w\"d \\\\ ผ่าน; -- `x` '")
        );
        // Planning made nothing.
        assert!(mysql_accounts(&mut admin, &p).await.is_empty());
        assert_eq!(session.create_user(&spec).await.unwrap(), masked);
        // The same name again: the server's refusal, and the first account untouched.
        assert!(matches!(
            session.create_user(&spec).await,
            Err(Error::Database(_))
        ));

        // It logs in with exactly that password, and with no other spelling of it.
        let mut as_app = mysql_as(&t, &app, HOSTILE).await.expect("app logs in");
        assert!(mysql_as(&t, &app, HOSTILE.trim()).await.is_err());
        assert!(mysql_as(&t, &app, "").await.is_err());
        // Everything inside its database...
        as_app
            .execute(format!("CREATE TABLE `{a}`.made (id INT PRIMARY KEY)").as_str())
            .await
            .unwrap();
        as_app
            .execute(format!("INSERT INTO `{a}`.made VALUES (7)").as_str())
            .await
            .unwrap();
        assert_eq!(
            mysql_count(&mut as_app, &format!("`{a}`.made"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            mysql_count(&mut as_app, &format!("`{a}`.t")).await.unwrap(),
            2
        );
        // ...and nothing outside it, not even where the name would match as a pattern.
        assert!(mysql_count(&mut as_app, &format!("`{b}`.t")).await.is_err());
        assert!(mysql_count(&mut as_app, &format!("`{wild}`.t"))
            .await
            .is_err());
        as_app.close().await.unwrap();

        // Read-only user.
        let spec = user(&reader, Some("%"), "readOnly", Some(&a), HOSTILE);
        let steps = session.create_user(&spec).await.unwrap();
        assert_eq!(
            steps[1].sql,
            format!("GRANT SELECT, SHOW VIEW ON `{escaped}`.* TO '{reader}'@'%'")
        );
        let mut as_reader = mysql_as(&t, &reader, HOSTILE)
            .await
            .expect("reader logs in");
        assert_eq!(
            mysql_count(&mut as_reader, &format!("`{a}`.t"))
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            mysql_count(&mut as_reader, &format!("`{a}`.v"))
                .await
                .unwrap(),
            2
        );
        for refused in [
            format!("INSERT INTO `{a}`.t VALUES (3, 'x')"),
            format!("UPDATE `{a}`.t SET note = 'x'"),
            format!("DELETE FROM `{a}`.t"),
            format!("CREATE TABLE `{a}`.nope (id INT)"),
            format!("DROP TABLE `{a}`.t"),
        ] {
            assert!(
                as_reader.execute(refused.as_str()).await.is_err(),
                "{refused}"
            );
        }
        assert!(mysql_count(&mut as_reader, &format!("`{b}`.t"))
            .await
            .is_err());
        as_reader.close().await.unwrap();
        assert_eq!(
            mysql_count(&mut admin, &format!("`{a}`.t")).await.unwrap(),
            2
        );

        // No privileges: it logs in and reads neither database.
        let spec = user(&bare, Some("%"), "none", None, HOSTILE);
        let steps = session.create_user(&spec).await.unwrap();
        assert_eq!(
            texts(&steps),
            [format!("CREATE USER '{bare}'@'%' IDENTIFIED BY {MASK}")]
        );
        let mut as_bare = mysql_as(&t, &bare, HOSTILE).await.expect("bare logs in");
        assert!(mysql_count(&mut as_bare, &format!("`{a}`.t"))
            .await
            .is_err());
        assert!(mysql_count(&mut as_bare, &format!("`{b}`.t"))
            .await
            .is_err());
        as_bare.close().await.unwrap();

        // The list and the details say what was granted.
        let listed = session.list_accounts().await.unwrap();
        for name in [&app, &reader, &bare] {
            let found = listed.iter().find(|x| &x.name == name).expect("listed");
            assert_eq!(found.host.as_deref(), Some("%"));
            assert!(!found.you && !found.system && !found.locked);
        }
        let details = session
            .account_details(&at(&reader, Some("%")))
            .await
            .unwrap();
        assert!(details.plugin.is_some() && !details.password_expired);
        assert!(
            details
                .grants
                .iter()
                .any(|g| g.starts_with("GRANT SELECT, SHOW VIEW ON") && g.contains(&escaped)),
            "{:?}",
            details.grants
        );
        assert!(details.grants.iter().all(|g| !g.contains("IDENTIFIED")));
        assert!(details.attributes.is_none() && details.databases.is_empty());
        assert!(matches!(
            session
                .account_details(&at("puricat_it_nobody", Some("%")))
                .await,
            Err(Error::NotFound(_))
        ));

        // Another password: the old one stops working, the new one works.
        let who = at(&app, Some("%"));
        assert_eq!(
            session.plan_password(&who, SECOND, false).await.unwrap(),
            format!("ALTER USER '{app}'@'%' IDENTIFIED BY {MASK}")
        );
        assert!(mysql_as(&t, &app, HOSTILE).await.is_ok());
        assert_eq!(
            session.change_password(&who, SECOND).await.unwrap(),
            format!("ALTER USER '{app}'@'%' IDENTIFIED BY {MASK}")
        );
        assert!(mysql_as(&t, &app, HOSTILE).await.is_err());
        assert!(mysql_as(&t, &app, SECOND).await.is_ok());

        // A grant the server refuses: the account that was just made is dropped again.
        let impossible = "x".repeat(70);
        let spec = user(
            &format!("{p}_gone"),
            Some("%"),
            "app",
            Some(&impossible),
            HOSTILE,
        );
        let error = session.create_user(&spec).await.unwrap_err().to_string();
        assert!(error.contains("Nothing was kept"), "{error}");
        assert!(!error.contains("p'w"), "{error}");
        assert!(!mysql_accounts(&mut admin, &p)
            .await
            .contains(&format!("{p}_gone")));

        // The accounts the forms leave alone.
        let own = at(&t.profile.username, me.host.as_deref());
        for (account, word) in [
            (&own, "logged in"),
            (&at("mysql.sys", Some("localhost")), "server's own"),
        ] {
            for result in [
                session.plan_drop_user(account).await,
                session.plan_password(account, "x", false).await,
                session.drop_user(account).await,
                session.change_password(account, "x").await,
            ] {
                let error = result.unwrap_err();
                assert!(matches!(error, Error::Invalid(_)), "{error}");
                assert!(error.to_string().contains(word), "{error}");
            }
        }

        // Drop: the exact statement, gone from the list, and gone once only.
        let who = at(&bare, Some("%"));
        assert_eq!(
            session.plan_drop_user(&who).await.unwrap(),
            format!("DROP USER '{bare}'@'%'")
        );
        assert!(mysql_accounts(&mut admin, &p).await.contains(&bare));
        session.drop_user(&who).await.unwrap();
        assert!(!mysql_accounts(&mut admin, &p).await.contains(&bare));
        assert!(session
            .list_accounts()
            .await
            .unwrap()
            .iter()
            .all(|x| x.name != bare));
        assert!(matches!(
            session.drop_user(&who).await,
            Err(Error::Database(_))
        ));

        // NO_BACKSLASH_ESCAPES for the session: a backslash is then written as itself. The
        // builder's statement for that mode, run on a connection in that mode, gives an
        // account that logs in with exactly the password; the other mode's spelling of the
        // same password would not.
        let mut strict = sqlx::MySqlConnection::connect_with(&mysql_options(&t))
            .await
            .unwrap();
        strict
            .execute("SET SESSION sql_mode = CONCAT(@@SESSION.sql_mode, ',NO_BACKSLASH_ESCAPES')")
            .await
            .unwrap();
        let mode: String = sqlx::query("SELECT @@SESSION.sql_mode")
            .fetch_one(&mut strict)
            .await
            .unwrap()
            .get(0);
        assert!(users::no_backslash_escapes(&mode), "{mode}");
        let facts = Facts {
            no_backslash_escapes: true,
            ..Default::default()
        };
        let literal = format!("{p}_lit");
        let spec = user(&literal, Some("%"), "none", None, HOSTILE);
        let steps = users::create_user(Engine::Mysql, &spec, &facts, Show::Run).unwrap();
        assert!(
            steps[0]
                .sql
                .ends_with("IDENTIFIED BY ' p''w\"d \\ ผ่าน; -- `x` '"),
            "{}",
            steps[0].sql
        );
        strict.execute(steps[0].sql.as_str()).await.unwrap();
        assert!(mysql_as(&t, &literal, HOSTILE).await.is_ok());
        let doubled = format!("{p}_dbl");
        let spec = user(&doubled, Some("%"), "none", None, HOSTILE);
        let wrong = users::create_user(Engine::Mysql, &spec, &Facts::default(), Show::Run).unwrap();
        strict.execute(wrong[0].sql.as_str()).await.unwrap();
        assert!(mysql_as(&t, &doubled, HOSTILE).await.is_err());
        strict.close().await.unwrap();

        session.close().await;
    })
    .await;

    for db in &dbs {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS `{db}`").as_str())
            .await;
    }
    for name in mysql_accounts(&mut admin, &prefix).await {
        let _ = admin
            .execute(format!("DROP USER IF EXISTS '{name}'@'%'").as_str())
            .await;
    }
    assert!(mysql_accounts(&mut admin, &prefix).await.is_empty());
    outcome.unwrap();
}

// ---- PostgreSQL ----------------------------------------------------------------------

fn pg_options(t: &Target, role: &str, database: &str) -> sqlx::postgres::PgConnectOptions {
    let mut opts = sqlx::postgres::PgConnectOptions::new()
        .host(&t.profile.host)
        .port(t.profile.port)
        .username(role)
        .database(database);
    if let (Some(pw), true) = (&t.password, role == t.profile.username) {
        opts = opts.password(pw);
    }
    opts
}

async fn pg_as(
    t: &Target,
    role: &str,
    database: &str,
) -> std::result::Result<sqlx::PgConnection, sqlx::Error> {
    sqlx::PgConnection::connect_with(&pg_options(t, role, database)).await
}

async fn pg_count(
    conn: &mut sqlx::PgConnection,
    table: &str,
) -> std::result::Result<i64, sqlx::Error> {
    let row = sqlx::query(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(conn)
        .await?;
    row.try_get(0)
}

async fn pg_roles(admin: &mut sqlx::PgConnection, prefix: &str) -> Vec<String> {
    sqlx::query(
        "SELECT rolname::text FROM pg_roles WHERE left(rolname, length($1)) = $1 ORDER BY 1",
    )
    .bind(prefix)
    .fetch_all(admin)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get::<String, _>(0))
    .collect()
}

/// The verifier the server keeps for a role (the administrator may read it).
async fn pg_verifier(admin: &mut sqlx::PgConnection, role: &str) -> Option<String> {
    sqlx::query("SELECT rolpassword FROM pg_authid WHERE rolname = $1")
        .bind(role)
        .fetch_one(admin)
        .await
        .unwrap()
        .get(0)
}

#[tokio::test]
async fn postgres_users() {
    let Some(t) = target(Engine::Postgres, "PURICAT_TEST_POSTGRES") else {
        eprintln!("skipped: PURICAT_TEST_POSTGRES is not set");
        return;
    };
    let me = t.profile.username.clone();
    let mut admin = pg_as(&t, &me, "postgres").await.unwrap();
    let p = scratch_prefix();
    let (a, b) = (format!("{p}_a"), format!("{p}_b"));
    let (prefix, dbs) = (p.clone(), [a.clone(), b.clone()]);

    let outcome = tokio::spawn(async move {
        let mut admin = pg_as(&t, &me, "postgres").await.unwrap();
        for db in [&a, &b] {
            admin
                .execute(format!("CREATE DATABASE \"{db}\"").as_str())
                .await
                .unwrap();
        }
        let mut in_a = pg_as(&t, &me, &a).await.unwrap();
        in_a.execute(
            "CREATE SCHEMA shop;
             CREATE TABLE public.t (id int PRIMARY KEY, note text);
             INSERT INTO public.t VALUES (1, 'one'), (2, 'two');
             CREATE TABLE shop.orders (id serial PRIMARY KEY, note text);
             INSERT INTO shop.orders (note) VALUES ('first');
             CREATE VIEW shop.v AS SELECT id FROM shop.orders",
        )
        .await
        .unwrap();
        let mut in_b = pg_as(&t, &me, &b).await.unwrap();
        in_b.execute("CREATE TABLE public.t (id int); INSERT INTO public.t VALUES (1)")
            .await
            .unwrap();
        in_b.close().await.unwrap();

        let session = Session::connect(&t.profile, t.password.as_deref())
            .await
            .unwrap();
        let (app, reader, bare) = (format!("{p}_app"), format!("{p}_reader"), format!("{p}_bare"));

        let before = session.list_accounts().await.unwrap();
        let own = before.iter().find(|x| x.you).expect("own role");
        assert_eq!(own.name, me);
        assert!(own.host.is_none() && !own.system);
        assert!(before.iter().all(|x| !x.name.starts_with("pg_")));

        // Application user: the preview names every schema the database has now.
        let spec = user(&app, None, "app", Some(&a), HOSTILE);
        let masked = session.plan_user(&spec, false).await.unwrap();
        let mut expected = vec![
            format!("CREATE ROLE \"{app}\" LOGIN PASSWORD {MASK}"),
            format!("GRANT CONNECT ON DATABASE \"{a}\" TO \"{app}\""),
        ];
        for schema in ["public", "shop"] {
            expected.extend([
                format!("GRANT USAGE, CREATE ON SCHEMA \"{schema}\" TO \"{app}\""),
                format!("GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA \"{schema}\" TO \"{app}\""),
                format!("GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA \"{schema}\" TO \"{app}\""),
                format!("ALTER DEFAULT PRIVILEGES IN SCHEMA \"{schema}\" GRANT ALL PRIVILEGES ON TABLES TO \"{app}\""),
                format!("ALTER DEFAULT PRIVILEGES IN SCHEMA \"{schema}\" GRANT ALL PRIVILEGES ON SEQUENCES TO \"{app}\""),
            ]);
        }
        assert_eq!(texts(&masked), expected);
        assert!(masked[..2].iter().all(|s| s.database.is_none()));
        assert!(masked[2..].iter().all(|s| s.database.as_deref() == Some(a.as_str())));
        let revealed = session.plan_user(&spec, true).await.unwrap();
        assert_eq!(
            revealed[0].sql,
            format!("CREATE ROLE \"{app}\" LOGIN PASSWORD ' p''w\"d \\ ผ่าน; -- `x` '")
        );
        assert!(pg_roles(&mut admin, &p).await.is_empty());
        assert_eq!(session.create_user(&spec).await.unwrap(), masked);
        assert!(pg_verifier(&mut admin, &app).await.is_some());
        // The same name again: refused by the server, the first role untouched.
        assert!(matches!(session.create_user(&spec).await, Err(Error::Database(_))));
        assert_eq!(pg_roles(&mut admin, &p).await, std::slice::from_ref(&app));

        // Everything inside its database: its tables, its sequences, new objects.
        let mut as_app = pg_as(&t, &app, &a).await.expect("app connects");
        as_app
            .execute("CREATE TABLE shop.made (id serial PRIMARY KEY)")
            .await
            .unwrap();
        as_app
            .execute("INSERT INTO shop.orders (note) VALUES ('second')")
            .await
            .unwrap();
        assert_eq!(pg_count(&mut as_app, "shop.orders").await.unwrap(), 2);
        assert_eq!(pg_count(&mut as_app, "public.t").await.unwrap(), 2);
        // A table the session's role makes later is covered by the default privileges.
        in_a.execute("CREATE TABLE shop.later (id int)").await.unwrap();
        as_app.execute("INSERT INTO shop.later VALUES (1)").await.unwrap();
        as_app.close().await.unwrap();
        // Nothing outside it: it may connect elsewhere (PUBLIC may), and reads nothing there.
        let mut elsewhere = pg_as(&t, &app, &b).await.expect("PUBLIC may connect");
        assert!(pg_count(&mut elsewhere, "public.t").await.is_err());
        elsewhere.close().await.unwrap();

        // Read-only user.
        let spec = user(&reader, None, "readOnly", Some(&a), HOSTILE);
        let steps = session.create_user(&spec).await.unwrap();
        assert_eq!(
            texts(&steps[2..5]),
            [
                format!("GRANT USAGE ON SCHEMA \"public\" TO \"{reader}\""),
                format!("GRANT SELECT ON ALL TABLES IN SCHEMA \"public\" TO \"{reader}\""),
                format!("ALTER DEFAULT PRIVILEGES IN SCHEMA \"public\" GRANT SELECT ON TABLES TO \"{reader}\""),
            ]
        );
        assert_eq!(steps.len(), 8);
        let mut as_reader = pg_as(&t, &reader, &a).await.expect("reader connects");
        assert_eq!(pg_count(&mut as_reader, "shop.orders").await.unwrap(), 2);
        assert_eq!(pg_count(&mut as_reader, "shop.v").await.unwrap(), 2);
        in_a.execute("CREATE TABLE shop.later2 (id int)").await.unwrap();
        assert_eq!(pg_count(&mut as_reader, "shop.later2").await.unwrap(), 0);
        for refused in [
            "INSERT INTO shop.orders (note) VALUES ('x')",
            "UPDATE shop.orders SET note = 'x'",
            "DELETE FROM shop.orders",
            "CREATE TABLE shop.nope (id int)",
            "CREATE TABLE public.nope (id int)",
            "DROP TABLE shop.orders",
        ] {
            assert!(as_reader.execute(refused).await.is_err(), "{refused}");
        }
        as_reader.close().await.unwrap();
        assert_eq!(pg_count(&mut in_a, "shop.orders").await.unwrap(), 2);

        // No privileges: it connects (PUBLIC may) and reads neither schema.
        let spec = user(&bare, None, "none", None, HOSTILE);
        let steps = session.create_user(&spec).await.unwrap();
        assert_eq!(texts(&steps), [format!("CREATE ROLE \"{bare}\" LOGIN PASSWORD {MASK}")]);
        let mut as_bare = pg_as(&t, &bare, &a).await.expect("bare connects");
        assert!(pg_count(&mut as_bare, "shop.orders").await.is_err());
        assert!(pg_count(&mut as_bare, "public.t").await.is_err());
        as_bare.close().await.unwrap();

        // The list and the details.
        let listed = session.list_accounts().await.unwrap();
        for name in [&app, &reader, &bare] {
            let found = listed.iter().find(|x| &x.name == name).expect("listed");
            assert!(!found.you && !found.system && !found.locked && !found.superuser);
        }
        admin
            .execute(format!("CREATE ROLE \"{p}_group\"; GRANT \"{p}_group\" TO \"{app}\"").as_str())
            .await
            .unwrap();
        let group = session.list_accounts().await.unwrap();
        let group = group.iter().find(|x| x.name == format!("{p}_group")).unwrap();
        assert!(group.locked, "a role that cannot log in");
        let details = session.account_details(&at(&app, None)).await.unwrap();
        let attributes = details.attributes.expect("attributes");
        assert!(attributes.login && !attributes.superuser && !attributes.create_role);
        assert_eq!(attributes.connection_limit, -1);
        assert_eq!(details.member_of, [format!("{p}_group")]);
        assert!(details.grants.is_empty() && details.plugin.is_none());
        let access = details.databases.iter().find(|d| d.name == a).expect("database a");
        assert!(access.connect && !access.owner && !access.create);
        let members = session.account_details(&at(&format!("{p}_group"), None)).await.unwrap();
        assert_eq!(members.members, std::slice::from_ref(&app));
        assert!(matches!(
            session.account_details(&at("puricat_it_nobody", None)).await,
            Err(Error::NotFound(_))
        ));

        // Another password: the verifier the server keeps is another one.
        let who = at(&app, None);
        let first = pg_verifier(&mut admin, &app).await;
        assert_eq!(
            session.plan_password(&who, SECOND, false).await.unwrap(),
            format!("ALTER ROLE \"{app}\" PASSWORD {MASK}")
        );
        assert_eq!(pg_verifier(&mut admin, &app).await, first);
        session.change_password(&who, SECOND).await.unwrap();
        let second = pg_verifier(&mut admin, &app).await;
        assert!(second.is_some() && second != first);

        // The exact password, both statements: the builder's text against the server's own
        // md5 of the same password sent as a parameter.
        let mut md5 = pg_as(&t, &me, "postgres").await.unwrap();
        md5.execute("SET password_encryption = 'md5'").await.unwrap();
        let facts = Facts {
            standard_strings: true,
            ..Default::default()
        };
        let exact = format!("{p}_exact");
        let spec = user(&exact, None, "none", None, HOSTILE);
        let steps = users::create_user(Engine::Postgres, &spec, &facts, Show::Run).unwrap();
        md5.execute(steps[0].sql.as_str()).await.unwrap();
        let matches = |password: &'static str| {
            sqlx::query(
                "SELECT rolpassword = 'md5' || md5($1 || rolname) FROM pg_authid WHERE rolname = $2",
            )
            .bind(password)
            .bind(exact.clone())
        };
        let same: bool = matches(HOSTILE).fetch_one(&mut md5).await.unwrap().get(0);
        assert!(same, "the stored password is the one that was typed");
        let trimmed: bool = matches(" p'w\"d \\ ผ่าน; -- `x`").fetch_one(&mut md5).await.unwrap().get(0);
        assert!(!trimmed);
        let sql = users::change_password(Engine::Postgres, &at(&exact, None), SECOND, &facts, Show::Run).unwrap();
        md5.execute(sql.as_str()).await.unwrap();
        let same: bool = matches(SECOND).fetch_one(&mut md5).await.unwrap().get(0);
        assert!(same);
        md5.close().await.unwrap();

        // A step inside the database that the server refuses: the role that had just been
        // created, and its right to connect, are taken back. Seen from a session that may
        // create roles and owns the database, but not the schema `shop`.
        let manager = format!("{p}_mgr");
        admin
            .execute(
                format!(
                    "CREATE ROLE \"{manager}\" LOGIN CREATEROLE;
                     ALTER DATABASE \"{a}\" OWNER TO \"{manager}\""
                )
                .as_str(),
            )
            .await
            .unwrap();
        let mut profile = t.profile.clone();
        profile.username = manager.clone();
        let limited = Session::connect(&profile, None).await.unwrap();
        let spec = user(&format!("{p}_gone"), None, "readOnly", Some(&a), HOSTILE);
        let error = limited.create_user(&spec).await.unwrap_err().to_string();
        assert!(error.contains("Nothing was kept"), "{error}");
        assert!(!error.contains("p'w"), "{error}");
        assert!(!pg_roles(&mut admin, &p).await.contains(&format!("{p}_gone")));
        limited.close().await;

        // The accounts the forms leave alone: this session's own, a superuser, the server's.
        admin
            .execute(format!("CREATE ROLE \"{p}_super\" SUPERUSER").as_str())
            .await
            .unwrap();
        for (account, word) in [
            (at(&me, None), "logged in"),
            (at(&format!("{p}_super"), None), "superuser"),
            (at("pg_monitor", None), "server's own"),
        ] {
            for result in [
                session.plan_drop_user(&account).await,
                session.plan_password(&account, "x", false).await,
                session.drop_user(&account).await,
                session.change_password(&account, "x").await,
            ] {
                let error = result.unwrap_err();
                assert!(matches!(error, Error::Invalid(_)), "{error}");
                assert!(error.to_string().contains(word), "{error}");
            }
        }

        // Drop. A role that owns a table is refused by the server and stays.
        let error = session.drop_user(&who).await.unwrap_err();
        assert!(matches!(error, Error::Database(_)), "{error}");
        assert!(pg_roles(&mut admin, &p).await.contains(&app));
        // One that holds nothing goes: the exact statement, gone from the list, once only.
        let who = at(&bare, None);
        assert_eq!(
            session.plan_drop_user(&who).await.unwrap(),
            format!("DROP ROLE \"{bare}\"")
        );
        session.drop_user(&who).await.unwrap();
        assert!(!pg_roles(&mut admin, &p).await.contains(&bare));
        assert!(session.list_accounts().await.unwrap().iter().all(|x| x.name != bare));
        assert!(matches!(session.drop_user(&who).await, Err(Error::Database(_))));

        in_a.close().await.unwrap();
        session.close().await;
    })
    .await;

    for db in &dbs {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)").as_str())
            .await;
    }
    for role in pg_roles(&mut admin, &prefix).await {
        let _ = admin
            .execute(format!("DROP ROLE IF EXISTS \"{role}\"").as_str())
            .await;
    }
    assert!(pg_roles(&mut admin, &prefix).await.is_empty());
    outcome.unwrap();
}

#[tokio::test]
async fn sqlite_has_no_users() {
    let path = std::env::temp_dir().join(format!("{}.db", scratch_prefix()));
    std::fs::write(&path, b"").unwrap();
    let profile: ConnectionProfile = serde_json::from_value(json!({
        "name": "file", "engine": "sqlite", "file": path, "connectTimeoutSecs": 1,
    }))
    .unwrap();
    let session = Session::connect(&profile.normalized().unwrap(), None)
        .await
        .unwrap();
    assert!(matches!(
        session.list_accounts().await,
        Err(Error::Unsupported(_))
    ));
    let spec = user("a", None, "none", None, "x");
    assert!(matches!(
        session.plan_user(&spec, false).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        session.create_user(&spec).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        session.plan_drop_user(&at("a", None)).await,
        Err(Error::Unsupported(_))
    ));
    session.close().await;
    let _ = std::fs::remove_file(path);
}
