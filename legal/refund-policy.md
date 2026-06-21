# Caesim Refund Policy

Effective date: 2026-06-21

This Refund Policy applies to purchases of prepaid Caesim credits.

This document is an operational draft and should be reviewed by qualified counsel before production launch.

## 1. Refund Requests

Customers can request a refund for a completed credit purchase with:

```bash
caesim credits purchases
caesim credits refund-request <purchase-id>
```

Submitting a request does not automatically issue a refund. Refund requests are reviewed before money is refunded through Stripe.

## 2. Unused Credits

Approved refunds normally reverse the corresponding unused credits. If you have already spent some or all credits from the purchase, the refund may be denied, partially approved, or require manual review.

## 3. Manual Review

If a Stripe refund succeeds but Caesim cannot safely reverse the matching credits, the refund event is marked for manual review.

Manual review may be needed when:

- Credits from the purchase were already consumed.
- The payment event cannot be matched.
- Stripe sends a partial or repeated refund event.
- Account or ledger records are inconsistent.

## 4. Timing

Stripe controls card-network refund timing after a refund is issued. Caesim cannot guarantee when funds will appear on your payment method.

## 5. Abuse

Caesim may deny refund requests connected to abuse, fraud, chargeback misuse, policy violations, or unlawful use.

## 6. Legal Rights

Nothing in this policy limits non-waivable consumer rights that apply under law.
