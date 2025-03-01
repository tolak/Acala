// This file is part of Acala.

// Copyright (C) 2020-2021 Acala Foundation.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::{
	dollar, AccountId, Amount, Balance, CdpEngine, CollateralCurrencyIds, Currencies, CurrencyId,
	DefaultSwapParitalPathList, DepositPerAuthorization, Dex, ExistentialDeposits, GetLiquidCurrencyId,
	GetNativeCurrencyId, GetStableCurrencyId, GetStakingCurrencyId, Honzon, Price, Rate, Ratio, Runtime,
	TradingPathLimit,
};

use super::utils::{feed_price, set_balance};
use core::convert::TryInto;
use frame_benchmarking::{account, whitelisted_caller};
use frame_system::RawOrigin;
use orml_benchmarking::runtime_benchmarks;
use orml_traits::{Change, GetByKey, MultiCurrencyExtended};
use runtime_common::{BNC, RENBTC, VSKSM};
use sp_runtime::{
	traits::{AccountIdLookup, One, StaticLookup, UniqueSaturatedInto},
	FixedPointNumber,
};
use sp_std::prelude::*;

const SEED: u32 = 0;

const NATIVE: CurrencyId = GetNativeCurrencyId::get();
const STABLECOIN: CurrencyId = GetStableCurrencyId::get();
const STAKING: CurrencyId = GetStakingCurrencyId::get();
const LIQUID: CurrencyId = GetLiquidCurrencyId::get();

const CURRENCY_LIST: [CurrencyId; 5] = [NATIVE, LIQUID, BNC, VSKSM, RENBTC];

fn inject_liquidity(
	maker: AccountId,
	currency_id_a: CurrencyId,
	currency_id_b: CurrencyId,
	max_amount_a: Balance,
	max_amount_b: Balance,
	deposit: bool,
) -> Result<(), &'static str> {
	// set balance
	<Currencies as MultiCurrencyExtended<_>>::update_balance(
		currency_id_a,
		&maker,
		max_amount_a.unique_saturated_into(),
	)?;
	<Currencies as MultiCurrencyExtended<_>>::update_balance(
		currency_id_b,
		&maker,
		max_amount_b.unique_saturated_into(),
	)?;

	let _ = Dex::enable_trading_pair(RawOrigin::Root.into(), currency_id_a, currency_id_b);

	Dex::add_liquidity(
		RawOrigin::Signed(maker.clone()).into(),
		currency_id_a,
		currency_id_b,
		max_amount_a,
		max_amount_b,
		Default::default(),
		deposit,
	)?;

	Ok(())
}

runtime_benchmarks! {
	{ Runtime, module_honzon }

	authorize {
		let caller: AccountId = whitelisted_caller();
		let to: AccountId = account("to", 0, SEED);
		let to_lookup = AccountIdLookup::unlookup(to);

		// set balance
		set_balance(NATIVE, &caller, DepositPerAuthorization::get());
	}: _(RawOrigin::Signed(caller), STAKING, to_lookup)

	unauthorize {
		let caller: AccountId = whitelisted_caller();
		let to: AccountId = account("to", 0, SEED);
		let to_lookup = AccountIdLookup::unlookup(to);

		// set balance
		set_balance(NATIVE, &caller, DepositPerAuthorization::get());
		Honzon::authorize(
			RawOrigin::Signed(caller.clone()).into(),
			STAKING,
			to_lookup.clone()
		)?;
	}: _(RawOrigin::Signed(caller), STAKING, to_lookup)

	unauthorize_all {
		let c in 0 .. CollateralCurrencyIds::get().len().saturating_sub(1) as u32;

		let caller: AccountId = whitelisted_caller();
		let currency_ids = CollateralCurrencyIds::get();
		let to: AccountId = account("to", 0, SEED);
		let to_lookup = AccountIdLookup::unlookup(to);

		// set balance
		set_balance(NATIVE, &caller, DepositPerAuthorization::get().saturating_mul(c.into()));
		for i in 0 .. c {
			Honzon::authorize(
				RawOrigin::Signed(caller.clone()).into(),
				currency_ids[i as usize],
				to_lookup.clone(),
			)?;
		}
	}: _(RawOrigin::Signed(caller))

	// `adjust_loan`, best case:
	// adjust both collateral and debit
	adjust_loan {
		let caller: AccountId = whitelisted_caller();
		let currency_id: CurrencyId = CollateralCurrencyIds::get()[0];
		let collateral_price = Price::one();		// 1 USD
		let debit_value = 100 * dollar(STABLECOIN);
		let debit_exchange_rate = CdpEngine::get_debit_exchange_rate(currency_id);
		let debit_amount = debit_exchange_rate.reciprocal().unwrap().saturating_mul_int(debit_value);
		let debit_amount: Amount = debit_amount.unique_saturated_into();
		let collateral_value = 10 * debit_value;
		let collateral_amount = Price::saturating_from_rational(dollar(currency_id), dollar(STABLECOIN)).saturating_mul_int(collateral_value);

		// set balance
		set_balance(currency_id, &caller, collateral_amount + ExistentialDeposits::get(&currency_id));

		// feed price
		feed_price(vec![(currency_id, collateral_price)])?;

		// set risk params
		CdpEngine::set_collateral_params(
			RawOrigin::Root.into(),
			currency_id,
			Change::NoChange,
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(Some(Rate::saturating_from_rational(10, 100))),
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(debit_value * 100),
		)?;
	}: _(RawOrigin::Signed(caller), currency_id, collateral_amount.try_into().unwrap(), debit_amount)

	transfer_loan_from {
		let currency_id: CurrencyId = CollateralCurrencyIds::get()[0];
		let sender: AccountId = account("sender", 0, SEED);
		let sender_lookup = AccountIdLookup::unlookup(sender.clone());
		let receiver: AccountId = whitelisted_caller();
		let receiver_lookup = AccountIdLookup::unlookup(receiver.clone());

		let debit_value = 100 * dollar(STABLECOIN);
		let debit_exchange_rate = CdpEngine::get_debit_exchange_rate(currency_id);
		let debit_amount = debit_exchange_rate.reciprocal().unwrap().saturating_mul_int(debit_value);
		let debit_amount: Amount = debit_amount.unique_saturated_into();
		let collateral_value = 10 * debit_value;
		let collateral_amount = Price::saturating_from_rational(dollar(currency_id), dollar(STABLECOIN)).saturating_mul_int(collateral_value);

		// set balance
		set_balance(currency_id, &sender, collateral_amount + ExistentialDeposits::get(&currency_id));
		set_balance(NATIVE, &sender, DepositPerAuthorization::get());

		// feed price
		feed_price(vec![(currency_id, Price::one())])?;

		// set risk params
		CdpEngine::set_collateral_params(
			RawOrigin::Root.into(),
			currency_id,
			Change::NoChange,
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(Some(Rate::saturating_from_rational(10, 100))),
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(debit_value * 100),
		)?;

		// initialize sender's loan
		Honzon::adjust_loan(
			RawOrigin::Signed(sender.clone()).into(),
			currency_id,
			collateral_amount.try_into().unwrap(),
			debit_amount,
		)?;

		// authorize receiver
		Honzon::authorize(
			RawOrigin::Signed(sender.clone()).into(),
			currency_id,
			receiver_lookup,
		)?;
	}: _(RawOrigin::Signed(receiver), currency_id, sender_lookup)

	close_loan_has_debit_by_dex {
		let u in 2 .. TradingPathLimit::get() as u32;
		let currency_id: CurrencyId = CollateralCurrencyIds::get()[0];
		let sender: AccountId = whitelisted_caller();
		let maker: AccountId = account("maker", 0, SEED);
		let debit_value = 100 * dollar(STABLECOIN);
		let debit_exchange_rate = CdpEngine::get_debit_exchange_rate(currency_id);
		let debit_amount = debit_exchange_rate.reciprocal().unwrap().saturating_mul_int(debit_value);
		let debit_amount: Amount = debit_amount.unique_saturated_into();
		let collateral_value = 10 * debit_value;
		let collateral_amount = Price::saturating_from_rational(dollar(currency_id), dollar(STABLECOIN)).saturating_mul_int(collateral_value);

		// set balance
		set_balance(currency_id, &sender, collateral_amount + ExistentialDeposits::get(&currency_id));

		let mut path = vec![currency_id];
		for i in 2 .. u {
			inject_liquidity(
				maker.clone(),
				CURRENCY_LIST[i as usize - 2],
				*path.last().unwrap(),
				10_000 * dollar(CURRENCY_LIST[i as usize - 2]),
				10_000 * dollar(*path.last().unwrap()),
				false,
			)?;
			path.push(CURRENCY_LIST[i as usize - 2]);
		}
		inject_liquidity(
			maker.clone(),
			*path.last().unwrap(),
			STABLECOIN,
			10_000 * dollar(*path.last().unwrap()),
			debit_value * 100,
			false,
		)?;
		path.push(STABLECOIN);

		// feed price
		feed_price(vec![(currency_id, Price::one())])?;

		// set risk params
		CdpEngine::set_collateral_params(
			RawOrigin::Root.into(),
			currency_id,
			Change::NoChange,
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(Some(Rate::saturating_from_rational(10, 100))),
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(debit_value * 100),
		)?;

		// initialize sender's loan
		Honzon::adjust_loan(
			RawOrigin::Signed(sender.clone()).into(),
			currency_id,
			collateral_amount.try_into().unwrap(),
			debit_amount,
		)?;
	}: _(RawOrigin::Signed(sender), currency_id, collateral_amount, Some(path))

	close_loan_has_debit_by_dex_no_path {
		let currency_id: CurrencyId = LIQUID;
		let mut default_path: Vec<CurrencyId> = DefaultSwapParitalPathList::get().last().unwrap().clone();

		let sender: AccountId = whitelisted_caller();
		let maker: AccountId = account("maker", 0, SEED);
		let debit_value = 100 * dollar(STABLECOIN);
		let debit_exchange_rate = CdpEngine::get_debit_exchange_rate(currency_id);
		let debit_amount = debit_exchange_rate.reciprocal().unwrap().saturating_mul_int(debit_value);
		let debit_amount: Amount = debit_amount.unique_saturated_into();
		let collateral_value = 10 * debit_value;
		let collateral_amount = Price::saturating_from_rational(dollar(currency_id), dollar(STABLECOIN)).saturating_mul_int(collateral_value);
		// set balance and trading path
		set_balance(currency_id, &sender, (10 * collateral_amount) + ExistentialDeposits::get(&currency_id));

		default_path.insert(0, currency_id);
		for i in 0..default_path.len() {
			if i != 0 {
				inject_liquidity(maker.clone(), default_path[i], default_path[i-1], 10_000 * dollar(default_path[i]), 10_000 * dollar(default_path[i-1]), false)?;
			}
		}

		feed_price(vec![(STAKING, Price::one())])?;

		// set risk params
		CdpEngine::set_collateral_params(
			RawOrigin::Root.into(),
			currency_id,
			Change::NoChange,
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(Some(Rate::saturating_from_rational(10, 100))),
			Change::NewValue(Some(Ratio::saturating_from_rational(150, 100))),
			Change::NewValue(debit_value * 100),
		)?;

		// initialize sender's loan
		Honzon::adjust_loan(
			RawOrigin::Signed(sender.clone()).into(),
			currency_id,
			(10 * collateral_amount).try_into().unwrap(),
			debit_amount,
		)?;

	}: close_loan_has_debit_by_dex(RawOrigin::Signed(sender), currency_id, collateral_amount, None)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::benchmarking::utils::tests::new_test_ext;
	use orml_benchmarking::impl_benchmark_test_suite;

	impl_benchmark_test_suite!(new_test_ext(),);
}
