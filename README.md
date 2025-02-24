# Norrland

SQLx convenience macro for adding transaction support to a struct wrapping a pool.

## The problem

Let's say we have this database layer:

```rust,ignore
use sqlx::{PgPool, query, query_as};

struct MyDB {
    pool: PgPool,
}

impl MyDB {
    pub async fn get_user(&self, id: &str) -> Result<User, sqlx::Error> {
        let user = query_as("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        Ok(user)
    }
    pub async fn register_user(&self, user: &User) -> Result<(), sqlx::Error> {
        query("INSERT INTO users (id, name) VALUES ($1, $2)")
            .bind(user.id)
            .bind(user.name)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}
```

Let's say we now want to wrap the two operations in a transaction that checks if a user exists, and registers them if not.

It is not obvious how we would refactor the DB layer to allow opening a transaction and then make both of these calls inside it.

## The solution

The best solution I've found is to extract the layer into a trait and implement it for `&mut PgConnection` and `&PgPool`.

This allows you to call the DB layer methods on the pool itself, as well as a transaction opened from the pool.

```rust,ignore
use sqlx::{query, query_as, PgConnection, PgPool};

pub trait MyDB {
    async fn get_user(self, id: &str) -> Result<User, sqlx::Error>;
    async fn register_user(self, user: &User) -> Result<(), sqlx::Error>;
}

impl MyDB for &mut PgConnection {
    pub async fn get_user(self, id: &str) -> Result<User, sqlx::Error> {
        let user = query_as("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(self.as_mut())
            .await?;

        Ok(user)
    }
    pub async fn register_user(self, user: &User) -> Result<(), sqlx::Error> {
        query("INSERT INTO users (id, name) VALUES ($1, $2)")
            .bind(user.id)
            .bind(user.name)
            .execute(self.as_mut())
            .await?;

        Ok(())
    }
}
impl MyDB for &PgPool {
    pub async fn get_user(self, id: &str) -> Result<User, sqlx::Error> {
        let mut conn = self.acquire().await?;
        conn.get_user(id).await
    }
    pub async fn register_user(self, user: &User) -> Result<(), sqlx::Error> {
        let mut conn = self.acquire().await?;
        conn.register_user(id).await
    }
}
```

The problem we have now is that each method needs to be duplicated three times:

- Define it in the trait
- Make the "real" implementation in the connection impl
- Add a wrapper method in the pool impl

This is the reason `norrland` was made, to automate the generation of these three while only writing what looks like one database impl.

## The result

Code which is very similar to the first example, but expands to the solution for you.

```rust,ignore
use sqlx::{query, query_as, Postgres};

#[norrland(Postgres)]
impl MyDB {
    async fn get_user(self, id: &str) -> Result<User, sqlx::Error> {
        let user = query_as("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(self.as_mut())
            .await?;

        Ok(user)
    }
    async fn register_user(self, user: &User) -> Result<(), sqlx::Error> {
        query("INSERT INTO users (id, name) VALUES ($1, $2)")
            .bind(user.id)
            .bind(user.name)
            .execute(self.as_mut())
            .await?;

        Ok(())
    }
}
```
