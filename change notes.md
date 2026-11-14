# Unreleased

## Fixed

- **Download address check:** the check that only GitHub may serve an update or an add-on now reads the host correctly. An address such as `https://evil.example?x=.github.com` was accepted before; addresses with user info or a port are refused too. No known way to reach the flaw existed, since addresses come from a signed catalog or from GitHub's own answers.
