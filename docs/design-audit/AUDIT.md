# Ferrum API combined UX and accessibility audit

## Audit scope

Core native desktop flow at 1280 × 820: launch and empty state, saved request editing,
collection navigation, environment management, response inspection, and request history.

## User goal and accessibility target

A developer should be able to find or create a request, configure it confidently, execute it,
understand the result, manage variables, and return to previous work without accidental loss.
The target is keyboard-operable desktop software with readable type, visible focus, non-color-only
status communication, and controls large enough for low-precision pointing.

## Evidence and flow health

1. **Launch and create a request — poor.** Evidence: `02-current-empty.png`.
2. **Configure and inspect a response — needs major revision.** Evidence:
   `05-current-populated-response.png`.
3. **Manage an environment — critical.** Evidence: `06-current-populated-environment.png`.
4. **Review request history — poor.** Evidence: `07-current-history.png`.

## Strengths

- The dark canvas is calm and free from decorative noise.
- Method, response status, duration, and size are available at the point of work.
- Collections, environments, and history use consistent vocabulary.
- The request/response split matches the developer's mental model.
- Sensitive values are visually masked and stored outside SQLite.

## Complete issue inventory

### Structure and navigation

1. Collections, Environments, and History switch only the sidebar, leaving an unrelated request
   editor dominant; this is especially damaging for environment editing.
2. The sidebar occupies 28–30% of the window even when empty, while dense forms inside it clip.
3. Navigation is rendered as three small text tabs without section counts or a strong location cue.
4. Creation forms are permanently exposed in the sidebar, adding clutter and providing no context.
5. Empty collection, environment, and history states lack explanation and a clear primary action.
6. Search claims a Command-K shortcut that is not implemented and does not explain its scope.
7. The top environment selector and sidebar environment selection duplicate state without explaining
   which one controls request execution.
8. Collection rows use tiny disclosure/square glyphs and weak indentation, making hierarchy hard
   to scan.

### Request tabs and loss prevention

9. Request tabs look like filled buttons rather than a persistent document tab strip.
10. The close control renders as an ambiguous tiny square and falls below a comfortable target.
11. Closing a dirty request discards work without warning.
12. The add-tab control is similarly tiny and visually detached from the tab set.
13. Dirty state is represented only by a small bullet and has no explanatory tooltip or close guard.
14. Long tab titles have no truncation or overflow strategy.
15. Active and inactive tabs rely too heavily on orange fill instead of position, border, and text
    hierarchy.

### Request configuration

16. Method/URL and Send are split over two rows, increasing eye travel and wasting vertical space.
17. Save, request name, and collection placement do not communicate whether they describe or act
    on the request.
18. Request name is an unlabeled text field.
19. The collection selector clips names and looks disabled because of its flat gray fill.
20. Send is available without an actionable inline URL validation message.
21. Request errors and success messages appear only in a tiny bottom status bar and are easy to
    miss.
22. Editing the request name or collection does not mark the tab dirty.
23. Sending autosaves in storage but leaves the visible tab dirty and navigation snapshot stale.

### Params, headers, and body editing

24. The Params/Headers/Body tabs repeat the button-like active treatment and have no row counts.
25. Key/value inputs use only a small fraction of the available width.
26. Checkboxes have no visible label, and field purpose is communicated mostly by placeholder text.
27. Remove and Add row controls look disabled, are undersized, and use ambiguous glyphs.
28. New rows default to disabled, an unexpected behavior that makes newly entered data silently
    inactive.
29. Row actions and column headings do not align consistently.
30. Body content type and editor controls have weak grouping and no validation feedback.

### Response inspection

31. Request and response regions use a fixed split; developers cannot allocate space based on body
    size or editing task.
32. Response tabs are shown in the empty state even though they cannot do anything.
33. Status, time, and size are compressed into small inline text rather than a readable summary.
34. Response content uses small type and a fixed-height editor that leaves large areas unused.
35. The read-only body offers no explicit Copy action or body search.
36. The empty state is low contrast and does not lead back to the Send action.
37. Pretty, Raw, and Headers lack counts or content-type context.
38. Large-response truncation is a small warning instead of a prominent, actionable state.

### Environment management

39. The variable table is forced into the narrow sidebar; names and values visibly truncate.
40. Column labels, controls, and secret state collide at normal window size.
41. “Stored in OS” appears as field text and can be mistaken for a literal variable value.
42. Secret status relies on a tiny checkbox without a clear description.
43. Environment creation is an unexplained inline field rather than a focused workflow.
44. Save Environment is detached from the editor and has no dirty-state feedback.
45. No helper text explains current values, sensitive storage, or which environment is active.

### History

46. History entries are unstructured wrapped text rather than rows with method, status, URL, time,
    and duration columns.
47. Success/failure depends heavily on teal/red color; status words are absent.
48. Timestamps are missing even though chronology is the primary purpose of history.
49. Rows do not visibly communicate that clicking reopens a request.
50. There are no status or method filters, and search scope is ambiguous.

### Visual design and accessibility

51. Default text and monospace content are too small for sustained developer-tool use.
52. Placeholder, inactive tab, and secondary text contrast is weak against the dark canvas.
53. Many targets are roughly 18–26 px high, below a comfortable desktop accessibility target.
54. Focus appearance is not deliberately differentiated from selection.
55. Several controls use text glyphs as icons with no accessible visible label.
56. Status communication is inconsistent and sometimes color-only.
57. Keyboard hints show macOS notation even on Windows/Linux.
58. Only a hard-coded dark theme exists; system/light preference is ignored.
59. Spacing and corner treatment are inconsistent between the top bar, tabs, form controls, and
    data tables.
60. The brand is reduced to orange uppercase text and does not establish a coherent product tone.

## Implementation acceptance criteria

- Primary navigation changes the main work surface when appropriate.
- Request tabs read as tabs, include safe close behavior, and remain usable with long names.
- Method, URL, and Send form one clear execution toolbar.
- Params/headers/environment tables use available width and explicit labels/actions.
- Response height is user-resizable and includes copy/search tools.
- Environment variables edit in the main canvas with unambiguous secret handling.
- History provides explicit success/failure text, timestamps, and readable row hierarchy.
- All core controls have stronger contrast, visible focus, larger targets, and cross-platform hints.
- Empty, busy, success, warning, and error states are clear near the work they affect.

## Evidence limits

The screenshots confirm layout, hierarchy, density, contrast risks, and visible affordances. They
cannot prove screen-reader announcements, every keyboard traversal order, color contrast ratios
under all displays, or behavior at every OS scaling setting. Those require platform assistive-
technology and keyboard testing after implementation.
