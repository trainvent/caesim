# Caesim Privacy Policy

Effective date: 2026-06-21

This Privacy Policy explains what Caesim collects and how data is processed when you use the CLI, account system, credit system, and hosted features.

This document is an operational draft and should be reviewed by qualified counsel before production launch.

## 1. Data Caesim Processes

Caesim may process:

- Account data, such as email address, user id, authentication session, account status, and credit balance.
- Billing data, such as Stripe customer ids, Checkout session ids, payment event ids, refund request ids, purchase amounts, and credit grants.
- Local file metadata, such as file paths, image sizes, hashes, labels, reports, and match reasons.
- Image content when you use hosted Vision features such as `--find`.
- Diagnostic data, such as gateway errors, webhook event ids, and credit ledger entries.

## 2. Local Files And Reports

Caesim runs on your machine and can scan local folders you provide. Local reports may include file paths, match reasons, labels, destination paths, and credit usage metadata.

You are responsible for choosing which folders to scan.

## 3. Hosted Vision Processing

When you use Vision-powered features, Caesim may upload image content to hosted services and Google Cloud Vision for analysis. This may include images, labels, safe-search signals, text signals, and related metadata.

Do not use hosted Vision features on images you are not allowed to process with third-party services.

## 4. Payments

Caesim uses Stripe for payment processing. Caesim does not store full card numbers. Stripe may process payment method details, billing information, fraud signals, and transaction records under Stripe's own terms and privacy policy.

## 5. Authentication And Storage

Caesim uses Supabase for account authentication and account-related storage. The local CLI stores a session file on your device so you can stay signed in.

## 6. Sharing

Caesim shares data with service providers only as needed to operate the service, process payments, perform Vision analysis, authenticate users, provide support, prevent abuse, or comply with law.

## 7. Retention

Account, billing, credit ledger, payment, and refund records may be retained as needed for accounting, dispute handling, abuse prevention, and legal compliance.

Local reports and local session files remain on your machine until you remove them or sign out.

## 8. Your Choices

You can avoid hosted Vision processing by not using `--find` or other hosted analysis features.

You can sign out locally with:

```bash
caesim logout
```

For account deletion, data export, billing, or privacy requests, contact the published support channel for Caesim.

## 9. Security

Caesim uses access controls, signed webhooks, service secrets, and account sessions to protect operational data. No system is perfectly secure.
