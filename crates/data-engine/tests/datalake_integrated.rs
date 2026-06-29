use data_engine::datalake::Datalake;

#[tokio::test]
#[ignore]
async fn test_datalake_connect() {
    let lake = Datalake::new();
    let tbs = lake
        .list_all_tables()
        .await
        .expect("Connect to iceberg failed");
    dbg!(tbs);
}
