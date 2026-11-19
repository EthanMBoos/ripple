# Ripple: lending versus relocation (Figure 1)

Play [comparison.mp4](comparison.mp4) in a player supporting MP4/H.264, such as
QuickTime Player or VLC. The video is 1920 x 1080, 62.5 frames per second,
approximately 55 seconds, with no audio. No simulator, account, or API key is
needed.

This is a simulation of two fixed policies, not an LLM experiment. Both runs use
seed 1, 16 hubs, 80 aircraft, eight hours of ordinary requests, and another 30
minutes to finish. The same surge starts at minute 60 at hubs 6, 2, 5, 7, and
10. Each affected hub receives 16 extra trauma requests over 15 minutes; the
surge event and its permissions end at minute 120.

The video follows the five aircraft originally at hub 3. Blue identifies that
origin even after relocation. Other aircraft are hidden. Orange trails mark
lending trips; blue trails show other flights. Gray dots are clinics; white
squares are hospitals. Dashed outlines show the usual service reach of hubs 3
and 7 on the lending panel only. Each circle extends to its hub's farthest
assigned clinic (13 km). These are illustrative extents, not exclusive
boundaries: sites interleave, and neighboring aircraft can help serve them. The
outlines remain visible throughout the day and do not represent extra demand or
a hazard. The relocation panel instead labels all four receiving hubs: hub 2
receives two aircraft, and hubs 5, 6, and 7 receive one each. Labels appear as
aircraft reach their destinations and count cumulative arrivals, not idle
aircraft. The map is wider than Figure 1 to include all four hubs receiving
aircraft from hub 3.

Display simplifications:

- Aircraft within 500 m of a hub are shown in stationary slots above it. This
  suppresses loitering; it also simplifies arrivals and departures. It is a
  display rule, not a measurement of idle status.
- Aircraft markers do not rotate. Trails show the last two simulated minutes
  outside hub areas. Site markers do not indicate open requests.
- Lending colors are inferred from recorded home-to-home flight paths. In this
  unobstructed scenario, the farthest point from hub 3 identifies the visited
  site: all 100 trips match within 150 m, with the next site over 1 km farther
  away. Sixteen trips visit another hub's site. Both the outbound and return
  legs are orange; this is not an assignment log.
- Both panels use the same clock. Playback stays at 500 times real time from
  minute 60 to minute 510, with no mid-video pauses or speed changes. The final
  results are held for 1.2 seconds.

The result cards come from the recorded full-day summaries. Hub 3 has 96
requests: lending serves all 96; relocation serves 12 and leaves 84 unserved.
These counts are not weighted; the separate whole-day score is the weighted
on-time share over the entire region. Both regenerated minute-90 drawings match
the manuscript's Figure 1 sources exactly.

The review version omits author contact information to preserve anonymity.
