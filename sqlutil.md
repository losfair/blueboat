# Convenient SQL queries

Query recent logs for an app:

```sql
select * from applog where appid = '22dd6811-5f26-40a4-a2d4-8b86b4639b0e' and logtime > (select unix_timestamp() * 1000 - 60000);
```

Query unreferenced bundles:

```sql
select id from bundles where not exists (select * from apps where bundle_id = bundles.id) and createtime < (select unix_timestamp() * 1000 - 180000);
```

Delete unreferenced bundles:

```sql
delete from bundles where not exists (select * from apps where bundle_id = bundles.id) and createtime < (select unix_timestamp() * 1000 - 180000);
```

Delete expired KV entries:

```sql
delete from appkv where (appexpiration <> 0 and appexpiration < (select unix_timestamp() * 1000))
```
