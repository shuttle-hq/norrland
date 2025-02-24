#![allow(async_fn_in_trait)]

use norrland::norrland;
use sqlx::{query, Acquire, Postgres};

#[norrland(Postgres)]
impl MyDBTrait for MyDB {
    pub async fn select(self, a: i32) -> Result<(), sqlx::Error> {
        query("SELECT num FROM numbers WHERE num = $1")
            .bind(a)
            .fetch_one(self.as_mut())
            .await?;

        Ok(())
    }
    #[tracing::instrument(skip_all)]
    pub async fn insert_with_internal_trx(self, a: i32) -> Result<(), sqlx::Error> {
        let mut trx = self.begin().await?;
        tracing::info!(value = a, "inserting value");
        query("INSERT INTO numbers VALUES ($1)")
            .bind(a)
            .execute(trx.as_mut())
            .await?;
        trx.commit().await?;

        Ok(())
    }
    pub async fn insert_with_internal_trx_fails(self, a: i32) -> Result<(), sqlx::Error> {
        let mut trx = self.begin().await?;
        query("INSERT INTO numbers VALUES ($1)")
            .bind(a)
            .execute(trx.as_mut())
            .await?;
        query("SELECT ???").execute(trx.as_mut()).await?; // fails
        trx.commit().await?;

        Ok(())
    }
    pub async fn noop(self) -> Result<(), sqlx::Error> {
        Ok(())
    }
    pub async fn multi_args(self, a: i32, b: &mut i32, mut c: String) -> Result<(), sqlx::Error> {
        c.push('a');
        let _ = a + *b + c.len() as i32;
        Ok(())
    }
    pub async fn destructure(
        self,
        _a1 @ A { b, c }: &A,
        // A { b: d, c: e }: A, // TODO
    ) -> Result<(), sqlx::Error> {
        let _ = *b + *c as i32;
        // let _ = d + e as i32;
        Ok(())
    }
    // TODO
    // pub async fn wild(self, _: bool) -> Result<(), sqlx::Error> {
    //     Ok(())
    // }
    pub async fn custom_error(self) -> Result<(), E> {
        Ok(())
    }
    #[allow(unused)]
    pub(crate) async fn public_restricted(self) -> Result<(), sqlx::Error> {
        Ok(())
    }
    // TODO: Allow private functions to move to private side trait MyDBInner and be called from the connection impl
    // pub async fn public(self) -> Result<(), sqlx::Error> {
    //     self.non_public().await?;
    //     Ok(())
    // }
    // async fn non_public(self) -> Result<(), sqlx::Error> {
    //     Ok(())
    // }
}
pub struct A {
    pub b: i32,
    pub c: bool,
}
pub enum E {
    Huh,
}
impl From<sqlx::Error> for E {
    fn from(_value: sqlx::Error) -> Self {
        Self::Huh
    }
}

#[cfg(test)]
mod tests {
    use super::{MyDB, MyDBTrait};

    use sqlx::{query, PgPool};
    use testcontainers_modules::{
        postgres::Postgres,
        testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt},
    };
    use tracing::Level;

    async fn setup() -> (ContainerAsync<Postgres>, PgPool) {
        let container = Postgres::default()
            .with_tag("16-alpine")
            .start()
            .await
            .unwrap();

        let conn_str = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            container.get_host().await.unwrap(),
            container.get_host_port_ipv4(5432).await.unwrap(),
        );
        let pool = PgPool::connect(&conn_str).await.unwrap();

        query("CREATE TABLE numbers (num INT)")
            .execute(&pool)
            .await
            .unwrap();

        (container, pool)
    }

    #[tokio::test]
    async fn external_plus_internal_trx() {
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
        let (_cont, pool) = setup().await;
        let db = MyDB::new(pool);

        // combine several db method calls in one transaction
        // transactions internal to db methods run inside this transaction
        let mut trx = db.pool.begin().await.unwrap();
        trx.as_mut().insert_with_internal_trx(1).await.unwrap();
        trx.as_mut().select(1).await.unwrap();
        trx.as_mut()
            .insert_with_internal_trx_fails(99)
            .await
            .unwrap_err();
        trx.as_mut().select(99).await.unwrap_err();
        trx.commit().await.unwrap();
        db.select(1).await.unwrap();

        // outer transaction rolls back
        let mut trx = db.pool.begin().await.unwrap();
        trx.as_mut().select(2).await.unwrap_err();
        trx.as_mut().insert_with_internal_trx(2).await.unwrap();
        trx.as_mut().select(2).await.unwrap();
        trx.as_mut().select(3).await.unwrap_err();
        trx.rollback().await.unwrap();
        db.select(2).await.unwrap_err();
    }
}
