CREATE TABLE `appkv` (
  `nsid` VARCHAR(64) NOT NULL ,
  `appkey` VARBINARY(1024) NOT NULL ,
  `appvalue` LONGBLOB NOT NULL ,
  `appmetadata` BLOB NOT NULL ,
  `appexpiration` BIGINT UNSIGNED NOT NULL );

ALTER TABLE `appkv` ADD PRIMARY KEY (`nsid`, `appkey`);
ALTER TABLE `appkv` ADD INDEX (`nsid`);
ALTER TABLE `appkv` ADD INDEX (`appexpiration`);
CREATE TABLE `applog` (
  `appid` VARCHAR(64) NOT NULL ,
  `logtime` BIGINT UNSIGNED NOT NULL ,
  `subid` INT UNSIGNED NOT NULL ,
  `logcontent` TEXT NOT NULL )
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `applog` ADD INDEX `time_range_query` (`appid`, `logtime`);

ALTER TABLE `applog` ADD PRIMARY KEY (`appid`, `logtime`, `subid`);

ALTER TABLE `applog` ADD INDEX (`logtime`);CREATE TABLE `apps` (
  `id` VARCHAR(64) NOT NULL ,
  `bundle_id` VARCHAR(64) NOT NULL ,
  `env` TEXT NOT NULL ,
  `kv_namespaces` TEXT NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL ,
  PRIMARY KEY (`id`))
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `apps` ADD INDEX (`bundle_id`);
CREATE TABLE `bundles` (
  `id` VARCHAR(64) NOT NULL ,
  `bundle` LONGBLOB NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL ,
  PRIMARY KEY (`id`))
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `bundles` ADD INDEX (`createtime`);
CREATE TABLE `routes` (
  `domain` VARCHAR(200) NOT NULL ,
  `path` VARCHAR(200) NOT NULL ,
  `appid` VARCHAR(64) NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL )
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `routes` ADD PRIMARY KEY (`domain`, `path`);

ALTER TABLE `routes` ADD INDEX (`domain`);

ALTER TABLE `routes` ADD INDEX (`appid`);
