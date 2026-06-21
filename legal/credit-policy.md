# Caesim Credit Policy

Effective date: 2026-06-21

This Credit Policy explains Caesim prepaid credits.

This document is an operational draft and should be reviewed by qualified counsel before production launch.

## 1. Price

Credits are sold in 1,000-credit packs.

The default price is:

```text
1,000 credits = $1.69 USD
```

The live price shown at checkout controls the purchase.

## 2. Usage Unit

The current Vision pricing model is:

```text
1 credit = 1 standard Vision image-feature operation
```

For example, scanning 57 images with one Vision feature uses 57 credits.

If a command uses multiple Vision features per image, credit usage may be higher.

## 3. Prepaid Balance

Credits are added after Stripe confirms payment and Caesim receives the payment webhook.

You can check your balance with:

```bash
caesim credits balance
```

## 4. No Cash Value

Credits are prepaid usage units. They are not cash, stored value, bank deposits, gift cards, or securities. Credits are not transferable and cannot be redeemed for cash except where required by law or under the Refund Policy.

## 5. Changes

Caesim may change credit pricing or usage rules in the future. The checkout summary and command prompt should show the current price before payment.

## 6. Failed Or Cancelled Checkout

If checkout is cancelled or payment fails, credits are not added.

## 7. Refunds

Refunds are governed by the Refund Policy. Approved refunds normally reverse the corresponding unused credits.
