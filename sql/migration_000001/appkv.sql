CREATE TABLE `appkv` (
  `nsid` VARCHAR(64) NOT NULL ,
  `appkey` VARBINARY(1024) NOT NULL ,
  `appvalue` LONGBLOB NOT NULL ,
  `appmetadata` BLOB NOT NULL ,
  `appexpiration` BIGINT UNSIGNED NOT NULL );

ALTER TABLE `appkv` ADD PRIMARY KEY (`nsid`, `appkey`);
ALTER TABLE `appkv` ADD INDEX (`nsid`);
ALTER TABLE `appkv` ADD INDEX (`appexpiration`);
