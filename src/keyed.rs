//! Keyed children: UI that follows a list of data and changes only what
//! changed.
//!
//! Bevy's UI is retained: an entity on screen stays until it's despawned, and
//! changing one of its components changes only that. What it doesn't do is
//! match fresh data to the entities already showing the old data. This does:
//! hand a parent a list of items, each with a key, and its children follow.
//!
//! ```ignore
//! commands.entity(grid).sync_children(inputs, |input| input.name.clone(), |row, _| {
//!     row.spawn(text("name"));
//!     row.meter(MeterConfig::new(0.0))
//!         .insert(Bind::new(|input: &Input, meter: &mut MeterValue| meter.0 = input.value));
//! });
//! ```
//!
//! - An item whose key is new is spawned by the closure, under an item root
//!   holding its key and its data.
//! - An item whose key was there before keeps its entities. If its data
//!   changed, its [`Bind`]s run and update its parts in place; if not,
//!   nothing at all happens to it.
//! - An item whose key is gone is despawned, and the rest are put in the
//!   list's order, moved rather than respawned.
//!
//! A new item's binds run once straight after it's spawned, so the closure
//! only builds its shape and each value is written in one place, its bind.
//! Binds are written in three ways: [`Bind::new`] edits a component in place,
//! [`Bind::insert`] gives the entity a component worked out from the data
//! when it differs, and [`Bind::entity`] does anything else.
//!
//! Item roots are ghost nodes: layout sees through them, so an item can be
//! several cells of a grid, or several rows of a column. Give each list a
//! parent of its own; the list's items become all of that parent's children.
//!
//! Syncing is queued like any command and runs at the next sync point, so a
//! system that syncs UI belongs before [`UiSystems::Prepare`], where what it
//! spawns is laid out the same frame.
//!
//! [`UiSystems::Prepare`]: bevy::ui::UiSystems::Prepare

use std::hash::Hash;
use std::sync::Arc;

use bevy::ecs::world::EntityWorldMut;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::ui::experimental::GhostNode;

/// An item root's key, which matches it to its data from one sync to the next.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Keyed<K: Send + Sync + 'static>(pub K);

/// An item root's data, as last synced. Read it to see what an item shows.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Item<T: Send + Sync + 'static>(T);

impl<T: Send + Sync + 'static> Item<T> {
    /// Returns a reference to the data this item shows.
    pub fn get(&self) -> &T {
        &self.0
    }
}

type BindFn<T> = dyn Fn(&T, &mut EntityWorldMut) + Send + Sync;

/// Updates the entity it's on from its item's data: when the item is spawned,
/// and each time its data changes. Put binds on any of an item's parts, the
/// item root included; an entity takes one, so combine several with [`Bind::and`].
#[derive(Component, Clone)]
pub struct Bind<T: Send + Sync + 'static>(Vec<Arc<BindFn<T>>>);

impl<T: Send + Sync + 'static> Bind<T> {
    /// Writes one component of this entity from the data.
    pub fn new<C: Component<Mutability = bevy::ecs::component::Mutable>>(
        apply: impl Fn(&T, &mut C) + Send + Sync + 'static,
    ) -> Self {
        Self::entity(move |data, entity| {
            if let Some(mut component) = entity.get_mut::<C>() {
                apply(data, &mut component);
            }
        })
    }

    /// Does anything to this entity with the data: insert components, or sync
    /// a list nested inside it.
    pub fn entity(apply: impl Fn(&T, &mut EntityWorldMut) + Send + Sync + 'static) -> Self {
        Self(vec![Arc::new(apply)])
    }

    /// Gives this entity a component worked out from the data, whenever it
    /// differs from the one it has: a text, a theme token, a value. Works for
    /// any component that can be compared, including touchy's theme bindings,
    /// which can only be replaced.
    pub fn insert<C: Component + PartialEq>(
        make: impl Fn(&T) -> C + Send + Sync + 'static,
    ) -> Self {
        Self::entity(move |data, entity| {
            let new = make(data);
            if entity.get::<C>() != Some(&new) {
                entity.insert(new);
            }
        })
    }

    /// This bind, then `next`.
    pub fn and(mut self, next: Self) -> Self {
        self.0.extend(next.0);
        self
    }
}

/// Keeps an entity's children matched to a list of keyed items.
pub trait SyncChildren {
    /// Matches the children to `items` by `key`, spawning new items with
    /// `spawn`. Keys must be unique within the list; a repeated key is skipped.
    fn sync_children<T, K>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
        K: Clone + Eq + Hash + Send + Sync + 'static;

    /// Matches the children to `items` by their place in the list: the item
    /// at each place keeps its entities and takes the new data there. For
    /// lists whose items have nothing better to be known by.
    fn sync_children_in_order<T>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let numbered: Vec<(usize, T)> = items.into_iter().enumerate().collect();
        let mut spawn = spawn;
        self.sync_children_numbered(numbered, move |children, item| spawn(children, item))
    }

    #[doc(hidden)]
    fn sync_children_numbered<T>(
        &mut self,
        numbered: Vec<(usize, T)>,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static;
}

impl SyncChildren for EntityCommands<'_> {
    fn sync_children<T, K>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
        K: Clone + Eq + Hash + Send + Sync + 'static,
    {
        let keyed: Vec<(K, T)> = items.into_iter().map(|item| (key(&item), item)).collect();
        let parent = self.id();
        self.commands()
            .queue(move |world: &mut World| sync(world, parent, keyed, spawn));
        self
    }

    fn sync_children_numbered<T>(
        &mut self,
        numbered: Vec<(usize, T)>,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let parent = self.id();
        self.commands()
            .queue(move |world: &mut World| sync(world, parent, numbered, spawn));
        self
    }
}

impl SyncChildren for EntityWorldMut<'_> {
    fn sync_children<T, K>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
        K: Clone + Eq + Hash + Send + Sync + 'static,
    {
        let keyed: Vec<(K, T)> = items.into_iter().map(|item| (key(&item), item)).collect();
        let parent = self.id();
        self.world_scope(|world| sync(world, parent, keyed, spawn));
        self
    }

    fn sync_children_numbered<T>(
        &mut self,
        numbered: Vec<(usize, T)>,
        spawn: impl FnMut(&mut ChildSpawnerCommands, &T) + Send + 'static,
    ) -> &mut Self
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let parent = self.id();
        self.world_scope(|world| sync(world, parent, numbered, spawn));
        self
    }
}

fn sync<T, K>(
    world: &mut World,
    parent: Entity,
    items: Vec<(K, T)>,
    mut spawn: impl FnMut(&mut ChildSpawnerCommands, &T),
) where
    T: Clone + PartialEq + Send + Sync + 'static,
    K: Clone + Eq + Hash + Send + Sync + 'static,
{
    if world.get_entity(parent).is_err() {
        return;
    }
    let children: Vec<Entity> = world
        .get::<Children>(parent)
        .map(|children| children.to_vec())
        .unwrap_or_default();
    let mut kept: HashMap<K, Entity> = children
        .iter()
        .filter_map(|&child| Some((world.get::<Keyed<K>>(child)?.0.clone(), child)))
        .collect();
    let mut order = Vec::with_capacity(items.len());
    let mut seen = HashSet::new();
    let mut to_bind = Vec::new();
    for (key, data) in items {
        if !seen.insert(key.clone()) {
            warn_once!("touchy: a synced list repeats a key; only its first item is shown");
        } else if let Some(item) = kept.remove(&key) {
            if world.get::<Item<T>>(item).is_none_or(|old| old.0 != data) {
                world.entity_mut(item).insert(Item(data));
                to_bind.push(item);
            }
            order.push(item);
        } else {
            let mut commands = world.commands();
            let mut root =
                commands.spawn((GhostNode, Keyed(key), Item(data.clone()), ChildOf(parent)));
            // A ghost node brings a `Transform` for worlds' sake, which under a UI
            // parent only earns a warning: UI places its children by layout.
            root.remove::<(Transform, GlobalTransform)>();
            root.with_children(|children| spawn(children, &data));
            let item = root.id();
            world.flush();
            to_bind.push(item);
            order.push(item);
        }
    }
    // Whatever's left over is no longer in the list, keyed or not.
    for stale in children.into_iter().filter(|child| !order.contains(child)) {
        world.entity_mut(stale).despawn();
    }
    world.entity_mut(parent).replace_children(&order);
    for item in to_bind {
        apply_binds::<T>(world, item);
    }
}

/// Runs every bind for `T` in an item, root included. It stops at items of
/// the same type nested inside, which bind to their own data.
fn apply_binds<T: Clone + Send + Sync + 'static>(world: &mut World, item: Entity) {
    let Some(data) = world.get::<Item<T>>(item).map(|item| item.0.clone()) else {
        return;
    };
    let mut pending = vec![item];
    let mut bound = Vec::new();
    while let Some(entity) = pending.pop() {
        if entity != item && world.get::<Item<T>>(entity).is_some() {
            continue;
        }
        if let Some(bind) = world.get::<Bind<T>>(entity) {
            bound.extend(bind.0.iter().map(|bind| (entity, bind.clone())));
        }
        if let Some(children) = world.get::<Children>(entity) {
            pending.extend(children.iter());
        }
    }
    for (entity, bind) in bound {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            bind(&data, &mut entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeTextColor, ThemeToken};

    #[derive(Clone, Debug, PartialEq)]
    struct Row {
        name: &'static str,
        value: f32,
    }

    fn row(name: &'static str, value: f32) -> Row {
        Row { name, value }
    }

    /// Counts the binds that ran, to show unchanged items cost nothing.
    #[derive(Resource, Default)]
    struct BindRuns(usize);

    /// The value a row's cell shows.
    #[derive(Component, Default, Debug, PartialEq)]
    struct Shown(f32);

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<BindRuns>();
        let list = app.world_mut().spawn(Node::default()).id();
        (app, list)
    }

    fn sync_rows(app: &mut App, list: Entity, rows: Vec<Row>) {
        app.world_mut().commands().entity(list).sync_children(
            rows,
            |row| row.name,
            |item, _| {
                item.spawn((
                    Shown::default(),
                    Bind::entity(|row: &Row, cell: &mut EntityWorldMut| {
                        cell.get_mut::<Shown>().unwrap().0 = row.value;
                        cell.world_scope(|world| world.resource_mut::<BindRuns>().0 += 1);
                    }),
                ));
            },
        );
        app.world_mut().flush();
    }

    /// Each item's key, cell entity and shown value, in order.
    fn rows(app: &mut App, list: Entity) -> Vec<(&'static str, Entity, f32)> {
        let world = app.world();
        world
            .get::<Children>(list)
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let cell = world.get::<Children>(item).unwrap()[0];
                        (
                            world.get::<Keyed<&'static str>>(item).unwrap().0,
                            cell,
                            world.get::<Shown>(cell).unwrap().0,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn runs(app: &mut App) -> usize {
        std::mem::take(&mut app.world_mut().resource_mut::<BindRuns>().0)
    }

    #[test]
    fn new_items_are_spawned_in_order_and_bound_once() {
        let (mut app, list) = app();
        sync_rows(&mut app, list, vec![row("threat", 0.8), row("cover", 0.3)]);
        let shown: Vec<_> = rows(&mut app, list)
            .into_iter()
            .map(|(key, _, value)| (key, value))
            .collect();
        assert_eq!(shown, [("threat", 0.8), ("cover", 0.3)]);
        assert_eq!(runs(&mut app), 2);
        let first = app.world().get::<Children>(list).unwrap()[0];
        assert!(
            app.world().get::<GhostNode>(first).is_some(),
            "layout sees through items"
        );
    }

    #[test]
    fn unchanged_items_cost_nothing_and_changed_ones_update_in_place() {
        let (mut app, list) = app();
        sync_rows(&mut app, list, vec![row("threat", 0.8), row("cover", 0.3)]);
        let before = rows(&mut app, list);
        runs(&mut app);

        sync_rows(&mut app, list, vec![row("threat", 0.8), row("cover", 0.3)]);
        assert_eq!(runs(&mut app), 0, "same data, no work");

        sync_rows(&mut app, list, vec![row("threat", 0.9), row("cover", 0.3)]);
        assert_eq!(runs(&mut app), 1, "only the changed item");
        let after = rows(&mut app, list);
        assert_eq!(after[0].1, before[0].1, "the same cell entity");
        assert_eq!(after[0].2, 0.9);
    }

    #[test]
    fn reordering_moves_items_and_gone_ones_are_despawned() {
        let (mut app, list) = app();
        sync_rows(
            &mut app,
            list,
            vec![row("a", 1.0), row("b", 2.0), row("c", 3.0)],
        );
        let cells: HashMap<_, _> = rows(&mut app, list)
            .into_iter()
            .map(|(key, cell, _)| (key, cell))
            .collect();

        sync_rows(
            &mut app,
            list,
            vec![row("c", 3.0), row("new", 4.0), row("a", 1.0)],
        );
        let after = rows(&mut app, list);
        let keys: Vec<_> = after.iter().map(|(key, ..)| *key).collect();
        assert_eq!(keys, ["c", "new", "a"]);
        assert_eq!(after[0].1, cells["c"], "moved, not respawned");
        assert_eq!(after[2].1, cells["a"]);
        assert!(app.world().get_entity(cells["b"]).is_err(), "b is gone");
    }

    #[test]
    fn inserted_components_change_only_when_they_differ() {
        let (mut app, list) = app();
        let sync = |app: &mut App, rows: Vec<Row>| {
            app.world_mut().commands().entity(list).sync_children(
                rows,
                |row| row.name,
                |item, _| {
                    item.spawn((
                        Text::default(),
                        Bind::insert(|row: &Row| Text::new(format!("{:.1}", row.value))).and(
                            Bind::insert(|row: &Row| {
                                ThemeTextColor(ThemeToken::new(if row.value > 0.5 {
                                    "hot"
                                } else {
                                    "cold"
                                }))
                            }),
                        ),
                    ));
                },
            );
            app.world_mut().flush();
        };
        let label = |app: &mut App| {
            let world = app.world_mut();
            let (text, color) = world
                .query::<(&Text, &ThemeTextColor)>()
                .single(world)
                .unwrap();
            (text.0.clone(), color.0.clone())
        };
        sync(&mut app, vec![row("a", 0.25)]);
        assert_eq!(label(&mut app), ("0.2".into(), ThemeToken::new("cold")));
        sync(&mut app, vec![row("a", 0.75)]);
        assert_eq!(label(&mut app), ("0.8".into(), ThemeToken::new("hot")));
    }

    #[test]
    fn a_repeated_key_shows_its_first_item_only() {
        let (mut app, list) = app();
        sync_rows(&mut app, list, vec![row("a", 1.0), row("a", 2.0)]);
        let shown: Vec<_> = rows(&mut app, list)
            .into_iter()
            .map(|(key, _, value)| (key, value))
            .collect();
        assert_eq!(shown, [("a", 1.0)]);
    }

    #[test]
    fn lists_in_order_are_matched_by_place() {
        let (mut app, list) = app();
        let spawn = |item: &mut ChildSpawnerCommands, _: &f32| {
            item.spawn((
                Shown::default(),
                Bind::new(|value: &f32, shown: &mut Shown| shown.0 = *value),
            ));
        };
        app.world_mut()
            .commands()
            .entity(list)
            .sync_children_in_order(vec![1.0, 2.0], spawn);
        app.world_mut().flush();
        let first_cell = |app: &App| {
            let item = app.world().get::<Children>(list).unwrap()[0];
            app.world().get::<Children>(item).unwrap()[0]
        };
        let before = first_cell(&app);
        app.world_mut()
            .commands()
            .entity(list)
            .sync_children_in_order(vec![5.0], spawn);
        app.world_mut().flush();
        assert_eq!(app.world().get::<Children>(list).unwrap().len(), 1);
        assert_eq!(first_cell(&app), before);
        assert_eq!(app.world().get::<Shown>(before), Some(&Shown(5.0)));
    }

    #[test]
    fn a_bind_can_sync_a_list_nested_in_its_item() {
        #[derive(Clone, Debug, PartialEq)]
        struct Card {
            name: &'static str,
            values: Vec<f32>,
        }
        let (mut app, list) = app();
        let sync_cards = |app: &mut App, cards: Vec<Card>| {
            app.world_mut().commands().entity(list).sync_children(
                cards,
                |card| card.name,
                |card, _| {
                    card.spawn((
                        Node::default(),
                        Bind::entity(|card: &Card, plots: &mut EntityWorldMut| {
                            plots.sync_children_in_order(card.values.clone(), |plot, _| {
                                plot.spawn((
                                    Shown::default(),
                                    Bind::new(|v: &f32, s: &mut Shown| s.0 = *v),
                                ));
                            });
                        }),
                    ));
                },
            );
            app.world_mut().flush();
        };
        sync_cards(
            &mut app,
            vec![Card {
                name: "throw",
                values: vec![0.5, 0.25],
            }],
        );
        sync_cards(
            &mut app,
            vec![Card {
                name: "throw",
                values: vec![0.75, 0.25],
            }],
        );
        let world = app.world_mut();
        let mut shown: Vec<f32> = world.query::<&Shown>().iter(world).map(|s| s.0).collect();
        shown.sort_by(f32::total_cmp);
        assert_eq!(shown, [0.25, 0.75], "two plots, updated in place");
    }
}
