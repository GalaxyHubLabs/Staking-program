use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};

#[cfg(not(feature = "no-entrypoint"))]
use solana_security_txt::security_txt;

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "NOVA AI Staking",
    project_url: "https://nova.galaxyhub.ai",
    contacts: "admin@orberai.xyz",
    policy: "https://www.nova.galaxyhub.ai/security-policy",
    preferred_languages: "en,es",
    source_code: "https://github.com/GalaxyHubLabs/NovaAI-Staking",
    auditors: "None"
}

declare_id!("6tPaSkVKra9nSFdvFEmiQKLH88WqeAzkjtWqgJAPn4N9");

const MIN_STAKE: u64 = 10_000_000_000;
const MIN_LOCK: u64 = 30;
const LOCK_STEP: u64 = 15;
const DAY_SECS: i64 = 86400;
const COOLDOWN_DAYS: i64 = 7;

#[program]
pub mod staking_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, bump: u8) -> Result<()> {
        let p = &mut ctx.accounts.pool_state;
        p.authority = ctx.accounts.authority.key();
        p.token_mint = ctx.accounts.token_mint.key();
        p.token_vault = ctx.accounts.token_vault.key();
        p.bump = bump;
        p.total_staked = 0;
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64, lock_days: u64) -> Result<()> {
        require!(amount >= MIN_STAKE, ErrorCode::MinStake);
        require!(lock_days >= MIN_LOCK, ErrorCode::MinLock);
        require!((lock_days - MIN_LOCK) % LOCK_STEP == 0, ErrorCode::BadLock);

        let clock = Clock::get()?;
        let s = &mut ctx.accounts.stake_account;
        
        s.user = ctx.accounts.user.key();
        s.amount = amount;
        s.start_time = clock.unix_timestamp;
        s.unlock_time = clock.unix_timestamp + (lock_days as i64 * DAY_SECS);
        s.lock_days = lock_days;
        s.is_active = true;
        s.cooldown_start = 0;
        s.bump = ctx.bumps.stake_account;

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token_account.to_account_info(),
                    to: ctx.accounts.token_vault.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount,
        )?;

        ctx.accounts.pool_state.total_staked = 
            ctx.accounts.pool_state.total_staked.saturating_add(amount);

        Ok(())
    }

    pub fn start_unstake(ctx: Context<StartUnstake>) -> Result<()> {
        let s = &mut ctx.accounts.stake_account;
        let clock = Clock::get()?;

        require!(s.is_active, ErrorCode::NotActive);
        require!(clock.unix_timestamp >= s.unlock_time, ErrorCode::Locked);
        require!(s.cooldown_start == 0, ErrorCode::CooldownOn);

        s.cooldown_start = clock.unix_timestamp;
        Ok(())
    }

    pub fn complete_unstake(ctx: Context<CompleteUnstake>) -> Result<()> {
        let s = &mut ctx.accounts.stake_account;
        let clock = Clock::get()?;

        require!(s.is_active, ErrorCode::NotActive);
        require!(s.cooldown_start > 0, ErrorCode::NoCooldown);
        require!(
            clock.unix_timestamp >= s.cooldown_start + (COOLDOWN_DAYS * DAY_SECS),
            ErrorCode::Wait7d
        );

        let seeds = &[
            b"pool_state",
            ctx.accounts.pool_state.token_mint.as_ref(),
            &[ctx.accounts.pool_state.bump],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.user_token_account.to_account_info(),
                    authority: ctx.accounts.pool_state.to_account_info(),
                },
                &[&seeds[..]],
            ),
            s.amount,
        )?;

        ctx.accounts.pool_state.total_staked = 
            ctx.accounts.pool_state.total_staked.saturating_sub(s.amount);
        s.is_active = false;

        Ok(())
    }

    pub fn restake(ctx: Context<Restake>, new_lock_days: u64) -> Result<()> {
        require!(new_lock_days >= MIN_LOCK, ErrorCode::MinLock);
        require!((new_lock_days - MIN_LOCK) % LOCK_STEP == 0, ErrorCode::BadLock);

        let s = &mut ctx.accounts.stake_account;
        let clock = Clock::get()?;

        require!(s.is_active, ErrorCode::NotActive);
        require!(clock.unix_timestamp >= s.unlock_time, ErrorCode::Locked);

        s.start_time = clock.unix_timestamp;
        s.unlock_time = clock.unix_timestamp + (new_lock_days as i64 * DAY_SECS);
        s.lock_days = new_lock_days;
        s.cooldown_start = 0;

        Ok(())
    }

    pub fn owner_deposit(ctx: Context<OwnerDeposit>, amount: u64) -> Result<()> {
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.owner_token_account.to_account_info(),
                    to: ctx.accounts.token_vault.to_account_info(),
                    authority: ctx.accounts.authority.to_account_info(),
                },
            ),
            amount,
        )
    }

    pub fn owner_withdraw(ctx: Context<OwnerWithdraw>, amount: u64) -> Result<()> {
        let seeds = &[
            b"pool_state",
            ctx.accounts.pool_state.token_mint.as_ref(),
            &[ctx.accounts.pool_state.bump],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.owner_token_account.to_account_info(),
                    authority: ctx.accounts.pool_state.to_account_info(),
                },
                &[&seeds[..]],
            ),
            amount,
        )
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + PoolState::INIT_SPACE,
        seeds = [b"pool_state", token_mint.key().as_ref()],
        bump
    )]
    pub pool_state: Account<'info, PoolState>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_mint: Account<'info, Mint>,
    #[account(
        constraint = token_vault.mint == token_mint.key(),
        constraint = token_vault.owner == pool_state.key(),
    )]
    pub token_vault: Account<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(
        init,
        payer = user,
        space = 8 + StakeAccount::INIT_SPACE,
        seeds = [b"stake", user.key().as_ref(), &pool_state.total_staked.to_le_bytes()],
        bump
    )]
    pub stake_account: Account<'info, StakeAccount>,
    #[account(mut)]
    pub pool_state: Account<'info, PoolState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == pool_state.token_mint,
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = token_vault.key() == pool_state.token_vault)]
    pub token_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StartUnstake<'info> {
    #[account(mut, constraint = stake_account.user == user.key())]
    pub stake_account: Account<'info, StakeAccount>,
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct CompleteUnstake<'info> {
    #[account(mut, constraint = stake_account.user == user.key())]
    pub stake_account: Account<'info, StakeAccount>,
    #[account(mut)]
    pub pool_state: Account<'info, PoolState>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == pool_state.token_mint,
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = token_vault.key() == pool_state.token_vault)]
    pub token_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Restake<'info> {
    #[account(mut, constraint = stake_account.user == user.key())]
    pub stake_account: Account<'info, StakeAccount>,
    pub user: Signer<'info>,
}

#[derive(Accounts)]
pub struct OwnerDeposit<'info> {
    #[account(constraint = pool_state.authority == authority.key())]
    pub pool_state: Account<'info, PoolState>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        constraint = owner_token_account.owner == authority.key(),
        constraint = owner_token_account.mint == pool_state.token_mint,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = token_vault.key() == pool_state.token_vault)]
    pub token_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct OwnerWithdraw<'info> {
    #[account(constraint = pool_state.authority == authority.key())]
    pub pool_state: Account<'info, PoolState>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        constraint = owner_token_account.owner == authority.key(),
        constraint = owner_token_account.mint == pool_state.token_mint,
    )]
    pub owner_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = token_vault.key() == pool_state.token_vault)]
    pub token_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[account]
#[derive(InitSpace)]
pub struct PoolState {
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub token_vault: Pubkey,
    pub total_staked: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct StakeAccount {
    pub user: Pubkey,
    pub amount: u64,
    pub start_time: i64,
    pub unlock_time: i64,
    pub lock_days: u64,
    pub is_active: bool,
    pub cooldown_start: i64,
    pub bump: u8,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Min 10T")]
    MinStake,
    #[msg("Min 30d")]
    MinLock,
    #[msg("15d+")]
    BadLock,
    #[msg("Locked")]
    Locked,
    #[msg("!Active")]
    NotActive,
    #[msg("CD on")]
    CooldownOn,
    #[msg("No CD")]
    NoCooldown,
    #[msg("Wait 7d")]
    Wait7d,
}
