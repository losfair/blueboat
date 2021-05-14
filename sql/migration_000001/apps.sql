CREATE TABLE `apps` (
  `id` VARCHAR(64) NOT NULL ,
  `bundle_id` VARCHAR(64) NOT NULL ,
  `env` TEXT NOT NULL ,
  `kv_namespaces` TEXT NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL ,
  PRIMARY KEY (`id`))
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `apps` ADD INDEX (`bundle_id`);
