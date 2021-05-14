CREATE TABLE `routes` (
  `domain` VARCHAR(200) NOT NULL ,
  `path` VARCHAR(200) NOT NULL ,
  `appid` VARCHAR(64) NOT NULL ,
  `createtime` BIGINT UNSIGNED NOT NULL )
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `routes` ADD PRIMARY KEY (`domain`, `path`);

ALTER TABLE `routes` ADD INDEX (`domain`);

ALTER TABLE `routes` ADD INDEX (`appid`);
