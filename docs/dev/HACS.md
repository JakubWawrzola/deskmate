# Default HACS list: submission

**Done: merged 2026-11-02 ([hacs/default#11979](https://github.com/hacs/default/pull/11979),
approved by frenck after one round of changes).** Deskmate is in the default
catalog; HACS picks it up when its catalog refreshes. Notes for the next
submission or for keeping the listing:

- The review found a real issue (node names from unauthenticated hello frames
  reaching Repairs); fixed in 0.8.4. Reviewers read the code, not only the
  checks.
- hassfest in their PR check needs exactly one `manifest.json` in the whole
  repository (the Stream Deck plugin's is `manifest.template.json` for that
  reason).
- An author without write access to `hacs/default` cannot re-request a review
  through the API; the button in the PR does it.

---

Goal: Deskmate shows up in HACS search without adding a custom repository.
Requirements: <https://www.hacs.xyz/docs/publish/include/>. Reviews take
weeks; after the merge HACS picks the repository up on its next scan.

## Repository state (done in 0.8.1)

- Public, issues enabled, not archived, description and topics set
  (`hacs`, `hacs-integration`, `home-assistant-integration`, ...).
- `hacs.json` with `name`; integration in `custom_components/deskmate_link`.
- Brand icon in `custom_components/deskmate_link/brand/icon.png` (HACS accepts
  it instead of an entry in home-assistant/brands).
- `manifest.json` in hassfest order, `after_dependencies: ["camera"]`.
- `.github/workflows/validate.yml`: HACS action (no `ignore`) and hassfest on
  every push, pull request and weekly. Both green.
- A full GitHub release created after the validation passed (v0.8.1).

## Opening the pull request (owner's personal account)

1. Fork `hacs/default`, create a branch from `master` (not `master` itself).
2. Add `"JakubWawrzola/deskmate"` to the file `integration` in alphabetical
   order (case-insensitive), not at the end. Keep the file valid JSON.
3. Open the PR with the body below, every box ticked and every link filled.
   Do not request reviews.

### PR body

```markdown
## Checklist

- [x] I've read the [publishing documentation](https://hacs.xyz/docs/publish/start).
- [x] I've added the [HACS action](https://hacs.xyz/docs/publish/action) to my repository.
- [x] (For integrations only) I've added the [hassfest action](https://developers.home-assistant.io/blog/2020/04/16/hassfest/) to my repository.
- [x] The actions are passing without any disabled checks in my repository.
- [x] I've added a link to the action run on my repository below in the links section.
- [x] I've created a new release of the repository after the validation actions were run successfully.

## Links

Link to current release: RELEASE_URL
Link to successful HACS action (without the `ignore` key): HACS_RUN_URL
Link to successful hassfest action (if integration): HASSFEST_RUN_URL
```

## After the merge

- README, GUIDE, LINK and AI-DEPLOY lead with searching HACS for Deskmate
  (done). GUIDE and AI-DEPLOY keep one sentence about the custom
  repository for the first day, until the catalog refresh lists it; drop it
  once `https://data-v2.hacs.xyz/integration/data.json` contains
  `JakubWawrzola/deskmate`.
- Keep the validation green: HACS removes repositories that stop passing.
