use super::*;
#[kani::proof]
fn prove_share_zero_and_full_redemption() {
    let tokens: i128 = kani::any();
    let shares: i128 = kani::any();
    kani::assume(tokens >= 0 && shares > 0);
    let balance = PoolBalance {
        tokens,
        shares,
        q4w: 0,
    };
    assert_eq!(balance.convert_to_tokens(shares), tokens);
    let empty = PoolBalance {
        tokens: 0,
        shares: 0,
        q4w: 0,
    };
    assert_eq!(empty.convert_to_shares(tokens), tokens);
    assert_eq!(empty.convert_to_tokens(shares), 0);
    let depleted = PoolBalance {
        tokens: 0,
        shares,
        q4w: 0,
    };
    assert_eq!(depleted.convert_to_shares(tokens), 0);
}

#[kani::proof]
fn prove_share_round_trip_and_available_tokens() {
    // Deliberately bounded nonlinear domain, not protocol balance limits.
    let tokens = kani::any::<u8>() as i128;
    let shares = kani::any::<u8>() as i128;
    let amount = kani::any::<u8>() as i128;
    let queued = kani::any::<u8>() as i128;
    kani::assume(tokens > 0 && shares > 0 && queued <= shares);
    let balance = PoolBalance {
        tokens,
        shares,
        q4w: queued,
    };
    let minted = balance.convert_to_shares(amount);
    let redeemed = balance.convert_to_tokens(minted);
    assert!(minted >= 0 && redeemed >= 0 && redeemed <= amount);
    let available = balance.non_queued_tokens();
    assert!(available >= 0 && available <= tokens);
    if queued == 0 {
        assert_eq!(available, tokens);
    }
    if queued == shares {
        assert_eq!(available, 0);
    }
}

#[kani::proof]
fn prove_deposit_and_queue_balances() {
    let tokens: i128 = kani::any();
    let shares: i128 = kani::any();
    let queued: i128 = kani::any();
    let added_tokens: i128 = kani::any();
    let added_shares: i128 = kani::any();
    kani::assume(tokens >= 0 && shares >= 0 && queued >= 0 && queued <= shares);
    kani::assume(added_tokens >= 0 && added_tokens <= i128::MAX - tokens);
    kani::assume(added_shares >= 0 && added_shares <= i128::MAX - shares);
    let mut balance = PoolBalance {
        tokens,
        shares,
        q4w: queued,
    };
    balance.deposit(added_tokens, added_shares);
    balance.queue_for_withdraw(added_shares);
    assert_eq!(balance.tokens - tokens, added_tokens);
    assert_eq!(balance.shares - shares, added_shares);
    assert_eq!(balance.q4w - queued, added_shares);
    assert!(balance.q4w <= balance.shares);
}

#[kani::proof]
fn prove_withdraw_and_dequeue_balances() {
    let tokens: i128 = kani::any();
    let shares: i128 = kani::any();
    let queued: i128 = kani::any();
    let payout: i128 = kani::any();
    let burn: i128 = kani::any();
    kani::assume(tokens >= 0 && shares >= 0 && queued >= 0 && queued <= shares);
    kani::assume(payout >= 0 && burn >= 0);
    let mut balance = PoolBalance {
        tokens,
        shares,
        q4w: queued,
    };
    let result = balance.withdraw_balance(payout, burn);
    assert_eq!(result.is_ok(), payout <= tokens && burn <= queued);
    if result.is_ok() {
        assert_eq!(balance.tokens + payout, tokens);
        assert_eq!(balance.shares + burn, shares);
        assert_eq!(balance.q4w + burn, queued);
        assert!(balance.tokens >= 0 && balance.q4w >= 0 && balance.q4w <= balance.shares);
    } else {
        assert_eq!(balance.tokens, tokens);
        assert_eq!(balance.shares, shares);
        assert_eq!(balance.q4w, queued);
    }
    let mut dequeue = PoolBalance {
        tokens,
        shares,
        q4w: queued,
    };
    let result = dequeue.dequeue_balance(burn);
    assert_eq!(result.is_ok(), burn <= queued);
    assert_eq!(dequeue.tokens, tokens);
    assert_eq!(dequeue.shares, shares);
    if result.is_ok() {
        assert_eq!(dequeue.q4w + burn, queued);
        assert!(dequeue.q4w >= 0 && dequeue.q4w <= shares);
    } else {
        assert_eq!(dequeue.q4w, queued);
    }
    kani::cover!(payout == tokens && burn == queued && balance.q4w == 0);
    kani::cover!(payout > tokens);
    kani::cover!(burn > queued);
}
