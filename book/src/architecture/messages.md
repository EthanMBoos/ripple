# Messages

Messages are one of several ways information moves between plugins. Within an
entity, autonomy passes commands to controllers and controllers to the motion
model through ports, decisions read the entity's belief, and sensors hand
readings to their own entity (see
[Truth, measurements, and belief](information-flow.md)). Messages are the path
for everything else: between plugins that are not wired together, between
entities, and to and from world plugins, with a network deciding who receives
each one and when. This is the Rust counterpart of the pub/sub section of the
[C++ data-flow guide](../appendix/cpp/data-flow.md), which has known
inaccuracies (its `LocalNetwork` is not a range-limited radio).

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
  messages in the same tick; the planned estimators will also run after
  delivery.
- A delayed delivery arrives in a later tick; it never reaches an earlier one.
- Delivery order follows entity and plugin order, never worker timing.
- Entities removed in a tick receive nothing afterwards, including deliveries
  already scheduled.

## Sensor readings inside an entity

Sensors can also queue entity-local readings with `publish_local`. These are
not network messages: they are delivered to the entity's own plugins in the
delivery step after networks, and decision plugins read them in the next tick
as observations. The planned Estimator reads readings by sensor name (see
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
