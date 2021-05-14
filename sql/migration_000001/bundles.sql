CREATE TABLE `bundles` (
  `id` VARCHAR(64) NOT NULL ,
  `bundle` LONGBLOB NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL ,
  PRIMARY KEY (`id`))
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `bundles` ADD INDEX (`createtime`);
