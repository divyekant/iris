---
id: fb-026
type: feature-brief
audience: marketing
topic: production-hardening
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Production Ready

## One-Liner

Iris is hardened for real deployments — non-root Docker, encrypted credentials, CI/CD pipeline, and security headers baked in.

## The Problem

Self-hosted software often means "works on my laptop." Moving to a real deployment exposes gaps: containers running as root, credentials stored in plaintext, no automated testing, missing security headers. You end up spending more time hardening the tool than using it.

## The Solution

Iris v0.4 ships production-ready out of the box. The Docker image runs as a non-root user. Credentials are encrypted at rest. Security headers (CSRF protection, content policies) are enabled by default. A full CI/CD pipeline with 1,184 tests ensures every release meets the bar. Rate limiting protects against abuse. You deploy it and move on.

## Key Benefits

- **Non-root Docker**: The container runs as an unprivileged user by default, following security best practices without extra configuration
- **Encrypted credentials**: Email account credentials and API keys are encrypted at rest — not sitting in plaintext config files
- **CI/CD pipeline**: Automated build, test, and release process ensures every version is validated before it ships
- **Security headers**: CSRF protection, content security policies, and standard hardening headers are enabled out of the box
- **Rate limiting**: Configurable per-endpoint rate limits protect your deployment from abuse and runaway automations

## How It Works (Simple)

Iris applies production security defaults automatically. When you run the Docker image, it starts as an unprivileged user with security headers enabled. Credentials are encrypted before they touch disk. The CI/CD pipeline runs 1,184 tests on every change, catching issues before they reach your deployment. Rate limiting kicks in automatically to protect the API.

## Suggested Messaging

**Headline**: "Self-hosted doesn't have to mean self-hardened. Iris ships production-ready."

**IT/DevOps pitch**: "Non-root containers, encrypted credentials, CSRF protection, rate limiting, and 1,184 automated tests. Iris v0.4 is built for real deployments, not just demos."

**Comparison pitch**: "Most self-hosted email tools leave security as an exercise for the reader. Iris bakes it in — so your deployment is hardened from the first docker compose up."

## Competitive Edge

Self-hosted email clients typically ship as developer tools first, with production hardening left to the user. Iris is the first to include non-root containers, encrypted credential storage, CSRF protection, configurable rate limiting, and a full CI/CD pipeline as defaults. You get the privacy benefits of self-hosting with the security posture of a managed service.
