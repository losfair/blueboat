# Blueboat Metadata Service

[Blueboat Metadata Service (MDS)](https://github.com/losfair/blueboat-mds) is a distributed metadata manager for multi-region deployment of Blueboat.

## MDS

Some concepts:

- Clusters: A *cluster* in MDS' context is the collection of a FoundationDB primary cluster and all of its replicas.
- Stores: A *store* is a logical, isolated subspace in a *cluster*, formed by prepending a prefix to keys.
- Users: A *user* is identified by an Ed25519 public key.
- Roles: MDS uses an RBAC (role-based access control) permission model. A *user* is assigned one or more *roles*, and a *store* is assigned one or more *role grants*. A *user* can access any *store* where the intersection of `user.roles` and `store.role_grants` is non-empty.

MDS' configuration, including clusters, stores and users, is stored in the same structure as "external" data and can be accessed using MDS itself. Bootstrapping access to MDS' own configuration involves the creation of a initial *cluster*, *store* and *user*, and requires the use of `fdbcli`.

Each MDS instance maintains connections to all FoundationDB primaries and replicas in all clusters.

## Blueboat

Blueboat is provided with a MDS store for bootstrap (`--mds`) on startup, and this bootstrap store contains *shards* and *config*. Bootstrap store does not need to be highly available, but needs to be accessible *most of the time* to ensure that new Blueboat instances can be started and existing Blueboat instances can get config updates in time.

Keys in the bootstrap store:

- `config/metadata-shard`: The name of the *shard* for storing Blueboat's *metadata*.
- `shards/*`: Configuration of *shards*.

A *shard* is a set of paths that can reach a single *store*. The configuration of a *shard* is a list of MDS URLs that specify the protocol (`wss`), the hostname (e.g. `usw-1.mds.example.com`) and the name of the *store* (e.g. `blueboat-shard-default`). It looks like:

```json
{
  "servers": [
    { "url": "wss://usw-1.mds.example.com/blueboat-shard-default" },
    { "url": "wss://sgp-1.mds.example.com/blueboat-shard-default" }
  ]
}
```

Shards can be referenced in user applications' metadata and used as a key-value store.

### Metadata shard

The *metadata shard* is used for coordinating Blueboat instances, and must be highly available.

- Task scheduler

Tasks submitted with `Background.atLeastOnce()` are scheduled with the *task scheduler*.

A task scheduler config has the following key format:

```
task-scheduler/us-east-1/name-of-the-task-scheduler/config
task-scheduler/us-east-1/name-of-the-task-scheduler/lock
```

The `config` key contains something like:

```json
{
  "servers":"kafkasvc.default.svc.cluster.local:9092",
  "topic":"com.example.blueboat.scheduler.01",
  "consumer_group":"com.example.blueboat.scheduler.01.consumer"
}
```

The `lock` key is used for distributed locking and usually should not be manually written to.
