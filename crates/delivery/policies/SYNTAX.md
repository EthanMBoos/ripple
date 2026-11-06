A policy is a YAML list of rules. Each rule has `when`, an event kind
(surge, shortage, closure, outage, weather, priority, rooftop_closed,
corridor) or `check_in` (every call with no new event), and `do`, a list of
steps. Each step is a map with one key:

- `reposition: {class: fixed_wing or multirotor, count: N, until: event_end}`:
  N idle vehicles of the class to each of the event's hubs, from the nearest
  hubs that can spare them (each keeps half its own, and the class must
  reach). With `until: event_end` they fly home once idle after the event;
  without it they stay.
- `transfer: {item: event_item or "O-", units: half_stock or quarter_stock or N, from: {nearest_with_stock: K}}`:
  that many units to each of the event's hubs from its K nearest hubs with
  spare (each keeps a quarter of its starting stock).
- `serve_from: {hubs: {nearest: K} or {nearest_with_stock: K}}`: let the K
  nearest hubs (or the nearest with spare of the event's item) serve each of
  the event's hubs' sites besides its own.
- `ground: {hubs: each_hub or event_hubs, class: event_class or fixed_wing or multirotor or any}`:
  no launches from the event's hubs until the event ends (`each_hub` gives one
  order per hub in turn, `event_hubs` one order naming them all).
- `reserve: {hub: each_hub, count: N, site: event_site}`: keep N vehicles at
  each of the event's hubs for the event's site until the event ends.
- `prioritize: {site: event_site or null, urgency: trauma or urgent or routine or null}`:
  serve those requests first until the event ends.
- `avoid: {}`: route every flight around the event's area until it ends
  (closure and corridor only).
- `use_pad: {}`: deliver to the event's site's ground pad by drop until it
  ends (rooftop_closed).
- `top_up: {below: quarter_stock or N, units: half_stock or N}`: `check_in`
  only; transfer that many units to every hub holding less of an item, from
  its nearest hub with spare.

Steps that act on one hub (reposition, transfer, serve_from, and ground or
reserve with `each_hub`) run for each of the event's hubs in turn, in the
order written; the rest run once after them.

Example:

```yaml
- when: closure
  do: [{avoid: {}}]
- when: check_in
  do: [{top_up: {below: quarter_stock, units: half_stock}}]
```
