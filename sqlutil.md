# Convenient SQL queries

Query recent logs for an app:

```sql
select * from applog where appid = '22dd6811-5f26-40a4-a2d4-8b86b4639b0e' and logtime > (select unix_timestamp() * 1000 - 60000);
```
