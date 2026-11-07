# Agreed add-on direction

These requirements describe the owners' direction for Toolboks. Current implementation
and boundaries are documented in [ADDON-ARCHITECTURE.md](ADDON-ARCHITECTURE.md).

- Toolboks is the frame for installable add-ons. Each workspace stays in the same
  window, and installed/enabled add-ons appear in a collapsible sidebar.
- The four original tools are packaged as separate preinstalled add-ons.
- Add-ons can style their content while sharing the host's standard background colors.
  The approved host direction uses graphite backgrounds, a dark purple accent and the
  original Toolboks app icon. Text should be clear and comfortable to read.
- The standalone Stratum remains separate. Its condensed Toolboks workspace is
  Disk Manager, with subdued "by stratum" attribution.
- A standard downloadable SDK provides integration metadata, compatibility rules,
  declared capabilities, examples, validation and packaging. Integration metadata and
  user preferences stay separate.
- Only trusted first-party records receive the Toolboks verified checkmark and the
  first positions in the add-on browser. A package cannot claim that status itself.
- Configuration and add-on choices persist in locally generated files. Preview data
  stays separate from the stable app. This replaces the historical no-config rule.
- The audience is ordinary users with above-average interest in their system: clear
  explanations and defaults, with deeper controls available when useful.
- Every workspace offers Help with full and current-tab guided tours. Tours open only
  when requested, highlight their subject and can be cancelled at any point.

Remaining product decisions include independent third-party native distribution,
authenticated publisher identity/signing, an online catalog, remote update policy,
portable settings, and migration of all original native-tool settings. The current
native provider contract is host-linked source integration; these later capabilities
must not be implied by the local package browser.

Changes are tested locally before proposing a pull request to `staging`. Publishing
releases and merging to `main` remain separate actions.
