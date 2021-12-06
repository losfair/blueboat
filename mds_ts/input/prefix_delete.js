let t = createPrimaryTransaction();
t.PrefixDelete(data.prefix);
t.Commit().Wait();
