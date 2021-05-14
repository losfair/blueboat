CREATE TABLE `applog` (
  `appid` VARCHAR(64) NOT NULL ,
  `logtime` BIGINT UNSIGNED NOT NULL ,
  `subid` INT UNSIGNED NOT NULL ,
  `logcontent` TEXT NOT NULL )
  CHARSET=utf8mb4 COLLATE utf8mb4_bin;

ALTER TABLE `applog` ADD INDEX `time_range_query` (`appid`, `logtime`);

ALTER TABLE `applog` ADD PRIMARY KEY (`appid`, `logtime`, `subid`);

ALTER TABLE `applog` ADD INDEX (`logtime`);