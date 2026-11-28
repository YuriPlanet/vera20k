# Cluster scatter distance

Phobos lets a BulletType set the distance bounds used to place each cluster
successor around the original impact (`Ext/BulletType/Body.cpp:46-47`,
consumer `Ext/Bullet/Hooks.cpp:352-378`, hook `0x46902C`).

```ini
[CLUSTERBULLET]          ; BulletType
ClusterScatter.Min=1.0   ; cells, default 1.0 (256 leptons)
ClusterScatter.Max=2.0   ; cells, default 2.0 (512 leptons)
```

## Behavior

The stock cluster loop detonates a `Cluster=` bullet at its impact, then draws
`RandomRanged(0x100, 0x200)` leptons and moves the copy of the impact that far
in a random direction (`0x00469057`, `0x0049F420`), repeating after each
detonation. `ClusterScatter.Min`/`Max` replace those two bounds; the values are
`Valueable<Leptons>`, authored in cells and converted to leptons, so
`ClusterScatter.Min=0.5` is 128 leptons. Every successor is placed around the
original impact, never around the previous cluster.

## Where VERA20K owns it

`rules::projectile_type::ProjectileType` carries `cluster_scatter_min` /
`cluster_scatter_max`; `sim::combat`'s `commit_projectile_detonations_inline`
passes them to `sim::projectile::projectile_next_cluster_coord`, which keeps
the native draw order (one `RandomRanged` draw, then the one raw direction
draw). Stock builds keep the constant `0x100`/`0x200` bounds.

## Residuals

- None known for the cluster loop. `BallisticScatter.Min`/`Max` from the same
  Phobos class are a separate chain.
