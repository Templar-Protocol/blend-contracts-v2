// Ghost state that tracks whether or not we
// checked the user's positions for an acceptable health factor
//
// only modified by `positions_hf_under` (via `set_checked`)
static mut GHOST_CHECKED: bool = false;

pub fn get_checked() -> bool {
    unsafe { GHOST_CHECKED }
}
pub fn set_checked() {
    unsafe { GHOST_CHECKED = true }
}

// We want to prove a universally quantified property
// in `user_health`, namely that
// GHOST_CHECKED
//   OR (forall i, (state.collateral.get(i) >= old(state).collateral.get(i))
//                 AND (state.liabilities.get(i) <= old(state).collateral.get(i))
//
// we can't prove universally quantified assertions directly, so instead we
// prove the property for an arbitrary `SKOLEM_I` (named for Skolem variables)
static mut SKOLEM_I: u32 = 0;

pub fn skolem_i() -> u32 {
    unsafe { SKOLEM_I }
}

pub fn init() {
    unsafe {
        SKOLEM_I = cvlr::nondet();
        GHOST_CHECKED = cvlr::nondet();
    }
}
