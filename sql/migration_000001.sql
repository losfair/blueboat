create table applog (
  apppath varchar(200) not null,
  appversion varchar(100) not null,
  reqid varchar(100) not null,
  msg text not null,
  logseq int not null,
  logtime datetime not null,
  primary key (reqid, logseq),
  index app (apppath, logtime)
);

alter table applog modify logtime datetime(6) not null;

create algorithm = merge view applog_managed as
  select * from applog where apppath LIKE "managed/%";
