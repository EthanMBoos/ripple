# Messages

Messages are one of several ways information moves between plugins. Within an
entity, autonomy passes commands to controllers and controllers to the motion
model through ports, decisions read the entity's belief, and sensors hand
readings to their own entity's estimators (see
[Truth, measurements, and belief](information-flow.md)). Messages are the path
for everything else: between plugins that are not wired together, between
entities, and to and from world plugins, with a network deciding who receives
each one and when.

## Publishing and subscribing

```text
 plugin ── publish(network, topic, value) ──▶ its own outbox
 plugin ── subscribe::<T>(network, topic) ──▶ its own subscription
 plugin ── receive::<T>(network, topic)   ◀── its own inbox (a queue)
```

Every plugin owns its `Messages`: outbox, subscriptions, and inbox. There is no
shared message bus that plugins write to directly, and every subscriber has its
own queue, so one plugin reading a message never removes it for another.
Messages are typed; a subscriber that expects a different type is an error.
Published values implement `serde::Serialize` so `--trace` can record payloads
when a network delivers them.

## Networks decide delivery

```text
 network phase (coordinator), for each network in name order:
   collect every outbox's messages for this network, in entity/plugin order
   for each message and each subscriber on that topic:
       network decides: drop, or deliver after a delay
   queue deliveries due now; keep later ones for a future tick
```

| Network | Delivers to |
| --- | --- |
| `GlobalNetwork` | Every subscriber, immediately |
| `LocalNetwork` | Subscribers on the same entity only |
| `SphereNetwork` | Subscribers within a range of the sender |

A network plugin only makes the delivery decision; the engine owns the queues,
their order, and their limits. A network is a normal plugin, so research
networks (loss, delay, bandwidth) are ordinary user plugins.

The engine finds each message's subscribers from an index it builds in each
phase that has messages, so a message costs only as many decisions as it has
subscribers, and a message nobody subscribes to costs nothing. A network that
delivers only within the sender's entity (LocalNetwork) routes with
`route_within_entity`, and sees only its own entity's subscribers. SCRIMMAGE
decides every publisher–subscriber pair, which grows with the square of the team
when every agent publishes a topic every agent subscribes to; the paper measures
this (Removing Inherited Scaling Limits).

## Timing

```text
 tick k:    plugin publishes ─▶ network phase routes it ─▶ inbox
            plugins that run after delivery in tick k can read it
 tick k+1:  plugins that run earlier (autonomy, controllers) read it
```

- A message is available once the network phase has delivered it. Whether the
  recipient reads it in the same tick depends on when the recipient runs:
  autonomy and controllers run before delivery, so they read it in tick k+1;
  metrics run after, so they can count a tick's collision and other event
  messages in the same tick; estimators also run after delivery, so they
  read a message in the tick it is delivered.
- A delayed delivery arrives in a later tick; it never reaches an earlier one.
- Delivery order follows entity and plugin order, never worker timing.
- Entities removed in a tick receive nothing afterwards, including deliveries
  already scheduled.

## Sensor readings inside an entity

Sensors hand readings to their own entity with `publish_reading`. These are
not network messages: no network sees them, and only the entity's estimators
read them, by sensor name, in the estimation step after networks (see
[Truth, measurements, and belief](information-flow.md)).

## Limits

Queues have fixed capacities (in `pubsub/messages.rs`). They are limits on
how many messages a queue can hold at once, not per-tick budgets:

| Queue | Capacity |
| --- | --- |
| One entity plugin's outbox (not shared with the entity's other plugins) | 1,024 |
| One world plugin's outbox | 65,536 |
| One subscriber's unread messages on one channel | 65,536 |
| One network's pending deliveries | 65,536 |

Exceeding a capacity stops the run with an error naming the plugin, network, or
subscriber.
