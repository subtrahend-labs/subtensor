use super::*;
use frame_system::pallet_prelude::BlockNumberFor;
use safe_math::*;
use share_pool::{SharePool, SharePoolDataOperations};
use sp_runtime::Saturating;
use sp_std::ops::Neg;
use substrate_fixed::types::{I64F64, I96F32, U64F64, U96F32, U110F18};

impl<T: Config> Pallet<T> {
    /// Retrieves the total alpha issuance for a given subnet.
    ///
    /// This function calculates the total alpha issuance by summing the alpha
    /// values from `SubnetAlphaIn` and `SubnetAlphaOut` for the specified subnet.
    ///
    /// # Arguments
    /// * `netuid` - The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64` - The total alpha issuance for the specified subnet.
    pub fn get_alpha_issuance(netuid: u16) -> u64 {
        SubnetAlphaIn::<T>::get(netuid).saturating_add(SubnetAlphaOut::<T>::get(netuid))
    }

    /// Calculates the price of alpha for a given subnet.
    ///
    /// This function determines the price of alpha by dividing the total TAO
    /// reserves by the total alpha reserves (`SubnetAlphaIn`) for the specified subnet.
    /// If the alpha reserves are zero, the function returns zero to avoid division by zero.
    ///
    /// # Arguments
    /// * `netuid` - The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `I96F32` - The price of alpha for the specified subnet.
    pub fn get_alpha_price(netuid: u16) -> U96F32 {
        if netuid == Self::get_root_netuid() {
            return U96F32::saturating_from_num(1.0); // Root.
        }
        if SubnetMechanism::<T>::get(netuid) == 0 {
            return U96F32::saturating_from_num(1.0); // Stable
        }
        if SubnetAlphaIn::<T>::get(netuid) == 0 {
            U96F32::saturating_from_num(0)
        } else {
            U96F32::saturating_from_num(SubnetTAO::<T>::get(netuid))
                .checked_div(U96F32::saturating_from_num(SubnetAlphaIn::<T>::get(netuid)))
                .unwrap_or(U96F32::saturating_from_num(0))
        }
    }

    pub fn get_moving_alpha_price(netuid: u16) -> U96F32 {
        let one = U96F32::saturating_from_num(1.0);
        if netuid == Self::get_root_netuid() {
            // Root.
            one
        } else if SubnetMechanism::<T>::get(netuid) == 0 {
            // Stable
            one
        } else {
            U96F32::saturating_from_num(SubnetMovingPrice::<T>::get(netuid))
        }
    }

    /// Computes the smoothing factor α for the exponential moving average (EMA)
    /// based on current pool liquidity.
    ///
    /// This function implements a custom curve:
    /// 1. If `l >= liquidity_scale_max`, returns α = 1.
    /// 2. Otherwise:
    ///    - Normalize `x = 2·l / liquidity_scale_max − 1`.
    ///    - Compute a cubic polynomial
    ///      `f(x) = (((7/2·x³ − 1)·x³ + 3/2)·x − 4)`.
    ///    - Take `|f(x)|`, ceiling it to an integer `exp_int`, and set
    ///      α = 10^(−exp_int).
    ///
    /// # Arguments
    /// * `l` – Current liquidity measure (√(TAO·α) after scaling).
    /// * `liquidity_scale_max` – Liquidity level at which α saturates to 1.
    ///
    /// # Returns
    /// * `U96F32` – The EMA weight α in the range [0, 1].
    pub fn compute_alpha_for_ema(l: U96F32, liquidity_scale_max: U96F32) -> U96F32 {
        if l >= liquidity_scale_max {
            return U96F32::saturating_from_num(1);
        }

        let i_l_max = I96F32::saturating_from_num(liquidity_scale_max);
        let i_l = I96F32::saturating_from_num(l);
        let neg_one = I96F32::from_num(-1);
        let two = I96F32::from_num(2);
        let a = I96F32::from_num(7).safe_div(two);
        let b = neg_one;
        let c = I96F32::from_num(3).safe_div(two);
        let d = neg_one.saturating_mul(I96F32::from_num(4));
        let x = (two.saturating_mul(i_l).safe_div(i_l_max)).saturating_add(neg_one);

        let x_cubed = x.saturating_mul(x).saturating_mul(x);
        let f_x = ((a.saturating_mul(x_cubed).saturating_add(b))
            .saturating_mul(x_cubed)
            .saturating_add(c))
        .saturating_mul(x)
        .saturating_add(d);

        let abs_f_x = f_x.saturating_abs();
        let exp = abs_f_x.ceil();

        let exp_int = exp.to_num::<u32>();
        let mut alpha = I96F32::saturating_to_num(1);
        let ten = I96F32::saturating_to_num(10);

        for _ in 0..exp_int {
            alpha = alpha.safe_div(ten);
        }

        U96F32::saturating_from_num(alpha)
    }

    /// Updates the stored “moving” alpha price for a subnet using a dynamic EMA.
    ///
    /// Steps performed:
    /// 1. Load raw TAO and α reserves (`SubnetTAO`, `SubnetAlphaIn`) and down-scale by 1e9 (to TAO units)
    /// 2. Compute the constant-product k = TAO_reserves·α_reserves, then
    ///    l = √k (with minimal epsilon).
    /// 3. Call `compute_alpha_for_ema(l, liquidity_scale_max)` to obtain α.
    /// 4. Blend current price (`get_alpha_price`) and previous moving price
    ///    (`get_moving_alpha_price`) as
    ///    `α·current + (1−α)·moving`.
    /// 5. Clamp the result to ≤ current price and write into `SubnetMovingPrice`.
    ///
    /// # Arguments
    /// * `netuid` – The subnet identifier whose price to update.
    ///
    /// # Effects
    /// * Writes a new `I96F32` into storage map `SubnetMovingPrice::<T>::insert(netuid, …)`.
    pub fn update_moving_price(netuid: u16) {
        let tao_reserves_rao = U96F32::saturating_from_num(SubnetTAO::<T>::get(netuid));
        let alpha_reserves_rao = U96F32::saturating_from_num(SubnetAlphaIn::<T>::get(netuid));
        let tao_reserves = tao_reserves_rao.safe_div(U96F32::saturating_from_num(1_000_000_000));
        let alpha_reserves =
            alpha_reserves_rao.safe_div(U96F32::saturating_from_num(1_000_000_000));

        let k = tao_reserves.saturating_mul(alpha_reserves);
        let epsilon: U96F32 = U96F32::from_num(0.0000001);
        let l = checked_sqrt(k, epsilon).unwrap_or(U96F32::from_num(0));
        let liquidity_scale_max = U96F32::saturating_from_num(LiquidityScaleMax::<T>::get(netuid));
        let alpha = Self::compute_alpha_for_ema(l, liquidity_scale_max);

        let one_minus_alpha: U96F32 = U96F32::saturating_from_num(1.0).saturating_sub(alpha);
        let moving_price = Self::get_moving_alpha_price(netuid);
        let current_price = Self::get_alpha_price(netuid);
        let weighted_current_price: U96F32 = alpha.saturating_mul(current_price);
        let weighted_current_moving: U96F32 = one_minus_alpha.saturating_mul(moving_price);

        let mut new_moving: I96F32 = I96F32::saturating_from_num(
            weighted_current_price.saturating_add(weighted_current_moving),
        );

        new_moving = new_moving.min(I96F32::from_num(current_price));
        SubnetMovingPrice::<T>::insert(netuid, new_moving);
    }

    /// Retrieves the global global weight as a normalized value between 0 and 1.
    ///
    /// This function performs the following steps:
    /// 1. Fetches the global weight from storage using the TaoWeight storage item.
    /// 2. Converts the retrieved u64 value to a fixed-point number (U96F32).
    /// 3. Normalizes the weight by dividing it by the maximum possible u64 value.
    /// 4. Returns the normalized weight as an U96F32 fixed-point number.
    ///
    /// The normalization ensures that the returned value is always between 0 and 1,
    /// regardless of the actual stored weight value.
    ///
    /// # Returns
    /// * `U96F32` - The normalized global global weight as a fixed-point number between 0 and 1.
    ///
    /// # Note
    /// This function uses saturating division to prevent potential overflow errors.
    pub fn get_tao_weight() -> U96F32 {
        // Step 1: Fetch the global weight from storage
        let stored_weight = TaoWeight::<T>::get();

        // Step 2: Convert the u64 weight to U96F32
        let weight_fixed = U96F32::saturating_from_num(stored_weight);

        // Step 3: Normalize the weight by dividing by u64::MAX
        // This ensures the result is always between 0 and 1
        weight_fixed.safe_div(U96F32::saturating_from_num(u64::MAX))
    }

    /// Sets the global global weight in storage.
    ///
    /// This function performs the following steps:
    /// 1. Takes the provided weight value as a u64.
    /// 2. Updates the TaoWeight storage item with the new value.
    ///
    /// # Arguments
    /// * `weight` - The new global weight value to be set, as a u64.
    ///
    /// # Effects
    /// This function modifies the following storage item:
    /// - `TaoWeight`: Updates it with the new weight value.
    ///
    /// # Note
    /// The weight is stored as a raw u64 value. To get the normalized weight between 0 and 1,
    /// use the `get_tao_weight()` function.
    pub fn set_tao_weight(weight: u64) {
        // Update the TaoWeight storage with the new weight value
        TaoWeight::<T>::set(weight);
    }

    /// Calculates the weighted combination of alpha and global tao for a single hotkey onet a subnet.
    ///
    pub fn get_stake_weights_for_hotkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: u16,
    ) -> (I64F64, I64F64, I64F64) {
        // Retrieve the global tao weight.
        let tao_weight = I64F64::saturating_from_num(Self::get_tao_weight());
        log::debug!("tao_weight: {:?}", tao_weight);

        // Step 1: Get stake of hotkey (neuron)
        let alpha_stake =
            I64F64::saturating_from_num(Self::get_inherited_for_hotkey_on_subnet(hotkey, netuid));
        log::debug!("alpha_stake: {:?}", alpha_stake);

        // Step 2: Get the global tao stake for the hotkey
        let tao_stake = I64F64::saturating_from_num(Self::get_tao_inherited_for_hotkey_on_subnet(
            hotkey, netuid,
        ));
        log::debug!("tao_stake: {:?}", tao_stake);

        // Step 3: Combine alpha and tao stakes
        let total_stake = alpha_stake.saturating_add(tao_stake.saturating_mul(tao_weight));
        log::debug!("total_stake: {:?}", total_stake);

        (total_stake, alpha_stake, tao_stake)
    }

    /// Calculates the weighted combination of alpha and global tao for hotkeys on a subnet.
    ///
    pub fn get_stake_weights_for_network(netuid: u16) -> (Vec<I64F64>, Vec<I64F64>, Vec<I64F64>) {
        // Retrieve the global tao weight.
        let tao_weight: I64F64 = I64F64::saturating_from_num(Self::get_tao_weight());
        log::debug!("tao_weight: {:?}", tao_weight);

        // Step 1: Get subnetwork size
        let n: u16 = Self::get_subnetwork_n(netuid);

        // Step 2: Get stake of all hotkeys (neurons) ordered by uid
        let alpha_stake: Vec<I64F64> = (0..n)
            .map(|uid| {
                if Keys::<T>::contains_key(netuid, uid) {
                    let hotkey: T::AccountId = Keys::<T>::get(netuid, uid);
                    I64F64::saturating_from_num(Self::get_inherited_for_hotkey_on_subnet(
                        &hotkey, netuid,
                    ))
                } else {
                    I64F64::saturating_from_num(0)
                }
            })
            .collect();
        log::debug!("alpha_stake: {:?}", alpha_stake);

        // Step 3: Calculate the global tao stake vector.
        // Initialize a vector to store global tao stakes for each neuron.
        let tao_stake: Vec<I64F64> = (0..n)
            .map(|uid| {
                if Keys::<T>::contains_key(netuid, uid) {
                    let hotkey: T::AccountId = Keys::<T>::get(netuid, uid);
                    I64F64::saturating_from_num(Self::get_tao_inherited_for_hotkey_on_subnet(
                        &hotkey, netuid,
                    ))
                } else {
                    I64F64::saturating_from_num(0)
                }
            })
            .collect();
        log::trace!("tao_stake: {:?}", tao_stake);

        // Step 4: Combine alpha and root tao stakes.
        // Calculate the weighted average of alpha and global tao stakes for each neuron.
        let total_stake: Vec<I64F64> = alpha_stake
            .iter()
            .zip(tao_stake.iter())
            .map(|(alpha_i, tao_i)| alpha_i.saturating_add(tao_i.saturating_mul(tao_weight)))
            .collect();
        log::trace!("total_stake: {:?}", total_stake);

        (total_stake, alpha_stake, tao_stake)
    }

    /// Calculates the total inherited stake (alpha) held by a hotkey on a network, considering child/parent relationships.
    ///
    /// This function performs the following steps:
    /// 1. Retrieves the initial alpha (stake) for the hotkey on the specified subnet.
    /// 2. Retrieves the list of children and parents for the hotkey on the subnet.
    /// 3. Calculates the alpha allocated to children:
    ///    a. For each child, computes the proportion of alpha to be allocated.
    ///    b. Accumulates the total alpha allocated to all children.
    /// 4. Calculates the alpha received from parents:
    ///    a. For each parent, retrieves the parent's stake on the subnet.
    ///    b. Computes the proportion of the parent's stake to be inherited.
    ///    c. Accumulates the total alpha inherited from all parents.
    /// 5. Computes the final inherited alpha by adjusting the initial alpha:
    ///    a. Subtracts the alpha allocated to children.
    ///    b. Adds the alpha inherited from parents.
    /// 6. Returns the final inherited alpha value.
    ///
    /// # Arguments
    /// * `hotkey` - AccountId of the hotkey whose total inherited stake is to be calculated.
    /// * `netuid` - Network unique identifier specifying the subnet context.
    ///
    /// # Returns
    /// * `u64` - The total inherited alpha for the hotkey on the subnet after considering the stakes
    ///           allocated to children and inherited from parents.
    ///
    /// # Note
    /// This function uses saturating arithmetic to prevent overflows.
    pub fn get_tao_inherited_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: u16) -> u64 {
        let initial_tao: U96F32 = U96F32::saturating_from_num(
            Self::get_stake_for_hotkey_on_subnet(hotkey, Self::get_root_netuid()),
        );

        // Initialize variables to track alpha allocated to children and inherited from parents.
        let mut tao_to_children: U96F32 = U96F32::saturating_from_num(0);
        let mut tao_from_parents: U96F32 = U96F32::saturating_from_num(0);

        // Step 2: Retrieve the lists of parents and children for the hotkey on the subnet.
        let parents: Vec<(u64, T::AccountId)> = Self::get_parents(hotkey, netuid);
        let children: Vec<(u64, T::AccountId)> = Self::get_children(hotkey, netuid);
        log::trace!(
            "Parents for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            parents
        );
        log::trace!(
            "Children for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            children
        );

        // Step 3: Calculate the total tao allocated to children.
        for (proportion, _) in children {
            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!(
                "Normalized proportion for child: {:?}",
                normalized_proportion
            );

            // Calculate the amount of tao to be allocated to this child.
            let tao_proportion_to_child: U96F32 =
                U96F32::saturating_from_num(initial_tao).saturating_mul(normalized_proportion);
            log::trace!("Tao proportion to child: {:?}", tao_proportion_to_child);

            // Add this child's allocation to the total tao allocated to children.
            tao_to_children = tao_to_children.saturating_add(tao_proportion_to_child);
        }
        log::trace!("Total tao allocated to children: {:?}", tao_to_children);

        // Step 4: Calculate the total tao inherited from parents.
        for (proportion, parent) in parents {
            // Retrieve the parent's total stake on this subnet.
            let parent_tao: U96F32 = U96F32::saturating_from_num(
                Self::get_stake_for_hotkey_on_subnet(&parent, Self::get_root_netuid()),
            );
            log::trace!(
                "Parent tao for parent {:?} on subnet {}: {:?}",
                parent,
                netuid,
                parent_tao
            );

            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!(
                "Normalized proportion from parent: {:?}",
                normalized_proportion
            );

            // Calculate the amount of tao to be inherited from this parent.
            let tao_proportion_from_parent: U96F32 =
                U96F32::saturating_from_num(parent_tao).saturating_mul(normalized_proportion);
            log::trace!(
                "Tao proportion from parent: {:?}",
                tao_proportion_from_parent
            );

            // Add this parent's contribution to the total tao inherited from parents.
            tao_from_parents = tao_from_parents.saturating_add(tao_proportion_from_parent);
        }
        log::trace!("Total tao inherited from parents: {:?}", tao_from_parents);

        // Step 5: Calculate the final inherited tao for the hotkey.
        let finalized_tao: U96F32 = initial_tao
            .saturating_sub(tao_to_children) // Subtract tao allocated to children
            .saturating_add(tao_from_parents); // Add tao inherited from parents
        log::trace!(
            "Finalized tao for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            finalized_tao
        );

        // Step 6: Return the final inherited tao value.
        finalized_tao.saturating_to_num::<u64>()
    }

    pub fn get_inherited_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: u16) -> u64 {
        // Step 1: Retrieve the initial total stake (alpha) for the hotkey on the specified subnet.
        let initial_alpha: U96F32 =
            U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(hotkey, netuid));
        log::debug!(
            "Initial alpha for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            initial_alpha
        );
        if netuid == 0 {
            return initial_alpha.saturating_to_num::<u64>();
        }

        // Initialize variables to track alpha allocated to children and inherited from parents.
        let mut alpha_to_children: U96F32 = U96F32::saturating_from_num(0);
        let mut alpha_from_parents: U96F32 = U96F32::saturating_from_num(0);

        // Step 2: Retrieve the lists of parents and children for the hotkey on the subnet.
        let parents: Vec<(u64, T::AccountId)> = Self::get_parents(hotkey, netuid);
        let children: Vec<(u64, T::AccountId)> = Self::get_children(hotkey, netuid);
        log::debug!(
            "Parents for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            parents
        );
        log::debug!(
            "Children for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            children
        );

        // Step 3: Calculate the total alpha allocated to children.
        for (proportion, _) in children {
            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!(
                "Normalized proportion for child: {:?}",
                normalized_proportion
            );

            // Calculate the amount of alpha to be allocated to this child.
            let alpha_proportion_to_child: U96F32 =
                U96F32::saturating_from_num(initial_alpha).saturating_mul(normalized_proportion);
            log::trace!("Alpha proportion to child: {:?}", alpha_proportion_to_child);

            // Add this child's allocation to the total alpha allocated to children.
            alpha_to_children = alpha_to_children.saturating_add(alpha_proportion_to_child);
        }
        log::debug!("Total alpha allocated to children: {:?}", alpha_to_children);

        // Step 4: Calculate the total alpha inherited from parents.
        for (proportion, parent) in parents {
            // Retrieve the parent's total stake on this subnet.
            let parent_alpha: U96F32 =
                U96F32::saturating_from_num(Self::get_stake_for_hotkey_on_subnet(&parent, netuid));
            log::trace!(
                "Parent alpha for parent {:?} on subnet {}: {:?}",
                parent,
                netuid,
                parent_alpha
            );

            // Convert the proportion to a normalized value between 0 and 1.
            let normalized_proportion: U96F32 = U96F32::saturating_from_num(proportion)
                .safe_div(U96F32::saturating_from_num(u64::MAX));
            log::trace!(
                "Normalized proportion from parent: {:?}",
                normalized_proportion
            );

            // Calculate the amount of alpha to be inherited from this parent.
            let alpha_proportion_from_parent: U96F32 =
                U96F32::saturating_from_num(parent_alpha).saturating_mul(normalized_proportion);
            log::trace!(
                "Alpha proportion from parent: {:?}",
                alpha_proportion_from_parent
            );

            // Add this parent's contribution to the total alpha inherited from parents.
            alpha_from_parents = alpha_from_parents.saturating_add(alpha_proportion_from_parent);
        }
        log::debug!(
            "Total alpha inherited from parents: {:?}",
            alpha_from_parents
        );

        // Step 5: Calculate the final inherited alpha for the hotkey.
        let finalized_alpha: U96F32 = initial_alpha
            .saturating_sub(alpha_to_children) // Subtract alpha allocated to children
            .saturating_add(alpha_from_parents); // Add alpha inherited from parents
        log::trace!(
            "Finalized alpha for hotkey {:?} on subnet {}: {:?}",
            hotkey,
            netuid,
            finalized_alpha
        );

        // Step 6: Return the final inherited alpha value.
        finalized_alpha.saturating_to_num::<u64>()
    }

    /// Checks if a specific hotkey-coldkey pair has enough stake on a subnet to fulfill a given decrement.
    ///
    /// This function performs the following steps:
    /// 1. Retrieves the current stake for the hotkey-coldkey pair on the specified subnet.
    /// 2. Compares this stake with the requested decrement amount.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `coldkey` - The account ID of the coldkey.
    /// * `netuid` - The unique identifier of the subnet.
    /// * `decrement` - The amount of stake to be potentially decremented.
    ///
    /// # Returns
    /// * `bool` - True if the account has enough stake to fulfill the decrement, false otherwise.
    ///
    /// # Note
    /// This function only checks the stake for the specific hotkey-coldkey pair, not the total stake of the hotkey or coldkey individually.
    pub fn has_enough_stake_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
        decrement: u64,
    ) -> bool {
        // Retrieve the current stake for this hotkey-coldkey pair on the subnet
        let current_stake =
            Self::get_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid);

        // Compare the current stake with the requested decrement
        // Return true if the current stake is greater than or equal to the decrement
        current_stake >= decrement
    }

    /// Retrieves the alpha (stake) value for a given hotkey and coldkey pair on a specific subnet.
    ///
    /// This function performs the following steps:
    /// 1. Takes the hotkey, coldkey, and subnet ID as input parameters.
    /// 2. Accesses the Alpha storage map to retrieve the stake value.
    /// 3. Returns the retrieved stake value as a u64.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey (neuron).
    /// * `coldkey` - The account ID of the coldkey (owner).
    /// * `netuid` - The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64` - The alpha (stake) value for the specified hotkey-coldkey pair on the given subnet.
    ///
    /// # Note
    /// This function retrieves the stake specific to the hotkey-coldkey pair, not the total stake of the hotkey or coldkey individually.
    pub fn get_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
    ) -> u64 {
        let alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.try_get_value(coldkey).unwrap_or(0)
    }

    /// Retrieves the total stake (alpha) for a given hotkey on a specific subnet.
    ///
    /// This function performs the following step:
    /// 1. Retrieves and returns the total alpha value associated with the hotkey on the specified subnet.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `netuid` - The unique identifier of the subnet.
    ///
    /// # Returns
    /// * `u64` - The total alpha value for the hotkey on the specified subnet.
    ///
    /// # Note
    /// This function returns the cumulative stake across all coldkeys associated with this hotkey on the subnet.
    pub fn get_stake_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: u16) -> u64 {
        // Retrieve and return the total alpha this hotkey owns on this subnet.
        // This value represents the sum of stakes from all coldkeys associated with this hotkey.
        TotalHotkeyAlpha::<T>::get(hotkey, netuid)
    }

    /// Increase hotkey stake on a subnet.
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `netuid` - The unique identifier of the subnet.
    /// * `amount` - The amount of alpha to be added.
    ///
    pub fn increase_stake_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: u16, amount: u64) {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.update_value_for_all(amount as i64);
    }

    /// Decrease hotkey stake on a subnet.
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `netuid` - The unique identifier of the subnet.
    /// * `amount` - The amount of alpha to be added.
    ///
    pub fn decrease_stake_for_hotkey_on_subnet(hotkey: &T::AccountId, netuid: u16, amount: u64) {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.update_value_for_all((amount as i64).neg());
    }

    /// Buys shares in the hotkey on a given subnet
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `coldkey` - The account ID of the coldkey (owner).
    /// * `netuid` - The unique identifier of the subnet.
    /// * `amount` - The amount of alpha to be added.
    ///
    pub fn increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64 {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        // We expect to add a positive amount here.
        let actual_alpha = alpha_share_pool.update_value_for_one(coldkey, amount as i64);

        // We should return a positive amount, or 0 if the operation failed.
        // e.g. the stake was removed due to precision issues.
        actual_alpha.max(0).unsigned_abs()
    }

    pub fn try_increase_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        netuid: u16,
        amount: u64,
    ) -> bool {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);
        alpha_share_pool.sim_update_value_for_one(amount as i64)
    }

    /// Sell shares in the hotkey on a given subnet
    ///
    /// The function updates share totals given current prices.
    ///
    /// # Arguments
    /// * `hotkey` - The account ID of the hotkey.
    /// * `coldkey` - The account ID of the coldkey (owner).
    /// * `netuid` - The unique identifier of the subnet.
    /// * `amount` - The amount of alpha to be added.
    ///
    pub fn decrease_stake_for_hotkey_and_coldkey_on_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
        amount: u64,
    ) -> u64 {
        let mut alpha_share_pool = Self::get_alpha_share_pool(hotkey.clone(), netuid);

        // We expect a negative value here
        let mut actual_alpha = 0;
        if let Ok(value) = alpha_share_pool.try_get_value(coldkey) {
            if value >= amount {
                actual_alpha =
                    alpha_share_pool.update_value_for_one(coldkey, (amount as i64).neg());
            }
        }

        // Get the negation of the removed alpha, and clamp at 0.
        // This ensures we return a positive value, but only if
        // `actual_alpha` was negative (i.e. a decrease in stake).
        actual_alpha.neg().max(0).unsigned_abs()
    }

    /// Calculates Some(Alpha) returned from pool by staking operation
    /// if liquidity allows that. If not, returns None.
    ///
    /// If new alpha_reserve is about to drop below DefaultMinimumPoolLiquidity,
    /// then don't do it.
    ///
    pub fn sim_swap_tao_for_alpha(netuid: u16, tao: u64) -> Option<u64> {
        // Step 1: Get the mechanism type for the subnet (0 for Stable, 1 for Dynamic)
        let mechanism_id: u16 = SubnetMechanism::<T>::get(netuid);
        // Step 2: Initialized vars.
        if mechanism_id == 1 {
            // Step 3.a.1: Dynamic mechanism calculations
            let tao_reserves: U110F18 = U110F18::saturating_from_num(SubnetTAO::<T>::get(netuid));
            let alpha_reserves: U110F18 =
                U110F18::saturating_from_num(SubnetAlphaIn::<T>::get(netuid));
            // Step 3.a.2: Compute constant product k = alpha * tao
            let k: U110F18 = alpha_reserves.saturating_mul(tao_reserves);

            // Calculate new alpha reserve
            let new_alpha_reserves: U110F18 =
                k.safe_div(tao_reserves.saturating_add(U110F18::saturating_from_num(tao)));

            // Step 3.a.3: Calculate alpha staked using the constant product formula
            // alpha_stake_recieved = current_alpha - (k / (current_tao + new_tao))
            if new_alpha_reserves >= DefaultMinimumPoolLiquidity::<T>::get() {
                Some(
                    alpha_reserves
                        .saturating_sub(new_alpha_reserves)
                        .saturating_to_num::<u64>(),
                )
            } else {
                None
            }
        } else {
            // Step 3.b.1: Stable mechanism, just return the value 1:1
            Some(tao)
        }
    }

    /// Calculates Some(Tao) returned from pool by unstaking operation
    /// if liquidity allows that. If not, returns None.
    ///
    /// If new tao_reserve is about to drop below DefaultMinimumPoolLiquidity,
    /// then don't do it.
    ///
    pub fn sim_swap_alpha_for_tao(netuid: u16, alpha: u64) -> Option<u64> {
        // Step 1: Get the mechanism type for the subnet (0 for Stable, 1 for Dynamic)
        let mechanism_id: u16 = SubnetMechanism::<T>::get(netuid);
        // Step 2: Swap alpha and attain tao
        if mechanism_id == 1 {
            // Step 3.a.1: Dynamic mechanism calculations
            let tao_reserves: U110F18 = U110F18::saturating_from_num(SubnetTAO::<T>::get(netuid));
            let alpha_reserves: U110F18 =
                U110F18::saturating_from_num(SubnetAlphaIn::<T>::get(netuid));
            // Step 3.a.2: Compute constant product k = alpha * tao
            let k: U110F18 = alpha_reserves.saturating_mul(tao_reserves);

            // Calculate new tao reserve
            let new_tao_reserves: U110F18 = k
                .checked_div(alpha_reserves.saturating_add(U110F18::saturating_from_num(alpha)))
                .unwrap_or(U110F18::saturating_from_num(0));

            // Step 3.a.3: Calculate alpha staked using the constant product formula
            // tao_recieved = tao_reserves - (k / (alpha_reserves + new_tao))
            if new_tao_reserves >= DefaultMinimumPoolLiquidity::<T>::get() {
                Some(
                    tao_reserves
                        .saturating_sub(new_tao_reserves)
                        .saturating_to_num::<u64>(),
                )
            } else {
                None
            }
        } else {
            // Step 3.b.1: Stable mechanism, just return the value 1:1
            Some(alpha)
        }
    }

    /// Swaps TAO for the alpha token on the subnet.
    ///
    /// Updates TaoIn, AlphaIn, and AlphaOut
    pub fn swap_tao_for_alpha(netuid: u16, tao: u64) -> u64 {
        if let Some(alpha) = Self::sim_swap_tao_for_alpha(netuid, tao) {
            // Step 4. Decrease Alpha reserves.
            SubnetAlphaIn::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(alpha);
            });
            // Step 5: Increase Alpha outstanding.
            SubnetAlphaOut::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(alpha);
            });
            // Step 6: Increase Tao reserves.
            SubnetTAO::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(tao);
            });
            // Step 7: Increase Total Tao reserves.
            TotalStake::<T>::mutate(|total| {
                *total = total.saturating_add(tao);
            });
            // Step 8. Increase total subnet TAO volume.
            SubnetVolume::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(tao.into());
            });
            // Step 9. Return the alpha received.
            alpha
        } else {
            0
        }
    }

    /// Swaps a subnet's Alpba token for TAO.
    ///
    /// Updates TaoIn, AlphaIn, and AlphaOut
    pub fn swap_alpha_for_tao(netuid: u16, alpha: u64) -> u64 {
        if let Some(tao) = Self::sim_swap_alpha_for_tao(netuid, alpha) {
            // Step 4: Increase Alpha reserves.
            SubnetAlphaIn::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(alpha);
            });
            // Step 5: Decrease Alpha outstanding.
            SubnetAlphaOut::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(alpha);
            });
            // Step 6: Decrease tao reserves.
            SubnetTAO::<T>::mutate(netuid, |total| {
                *total = total.saturating_sub(tao);
            });
            // Step 7: Reduce total TAO reserves.
            TotalStake::<T>::mutate(|total| {
                *total = total.saturating_sub(tao);
            });
            // Step 8. Increase total subnet TAO volume.
            SubnetVolume::<T>::mutate(netuid, |total| {
                *total = total.saturating_add(tao.into());
            });
            // Step 9. Return the tao received.
            tao
        } else {
            0
        }
    }

    /// Unstakes alpha from a subnet for a given hotkey and coldkey pair.
    ///
    /// We update the pools associated with a subnet as well as update hotkey alpha shares.
    pub fn unstake_from_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
        alpha: u64,
        fee: u64,
    ) -> u64 {
        // Step 1: Decrease alpha on subneet
        let actual_alpha_decrease =
            Self::decrease_stake_for_hotkey_and_coldkey_on_subnet(hotkey, coldkey, netuid, alpha);

        // Step 2: Swap the alpha for TAO.
        let tao: u64 = Self::swap_alpha_for_tao(netuid, actual_alpha_decrease);

        // Step 3: Update StakingHotkeys if the hotkey's total alpha, across all subnets, is zero
        // TODO const: fix.
        // if Self::get_stake(hotkey, coldkey) == 0 {
        //     StakingHotkeys::<T>::mutate(coldkey, |hotkeys| {
        //         hotkeys.retain(|k| k != hotkey);
        //     });
        // }

        // Step 4. Reduce tao amount by staking fee and credit this fee to SubnetTAO
        let tao_unstaked = tao.saturating_sub(fee);
        let actual_fee = tao.saturating_sub(tao_unstaked);
        SubnetTAO::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(actual_fee);
        });
        TotalStake::<T>::mutate(|total| {
            *total = total.saturating_add(actual_fee);
        });
        LastColdkeyHotkeyStakeBlock::<T>::insert(coldkey, hotkey, Self::get_current_block_as_u64());

        // Step 5. Deposit and log the unstaking event.
        Self::deposit_event(Event::StakeRemoved(
            coldkey.clone(),
            hotkey.clone(),
            tao_unstaked,
            actual_alpha_decrease,
            netuid,
            actual_fee,
        ));
        log::debug!(
            "StakeRemoved( coldkey: {:?}, hotkey:{:?}, tao: {:?}, alpha:{:?}, netuid: {:?}, fee: {:?} )",
            coldkey.clone(),
            hotkey.clone(),
            tao_unstaked,
            actual_alpha_decrease,
            netuid,
            actual_fee
        );

        // Step 6: Return the amount of TAO unstaked.
        tao_unstaked
    }

    /// Stakes TAO into a subnet for a given hotkey and coldkey pair.
    ///
    /// We update the pools associated with a subnet as well as update hotkey alpha shares.
    pub(crate) fn stake_into_subnet(
        hotkey: &T::AccountId,
        coldkey: &T::AccountId,
        netuid: u16,
        tao: u64,
        fee: u64,
    ) -> u64 {
        // Step 1. Reduce tao amount by staking fee and credit this fee to SubnetTAO
        // At this point tao was already withdrawn from the user balance and is considered
        // available
        let tao_staked = tao.saturating_sub(fee);
        let actual_fee = tao.saturating_sub(tao_staked);

        // Step 2. Swap the tao to alpha.
        let alpha: u64 = Self::swap_tao_for_alpha(netuid, tao_staked);
        let mut actual_alpha = 0;
        if (tao_staked > 0) && (alpha > 0) {
            // Step 3: Increase the alpha on the hotkey account.
            actual_alpha = Self::increase_stake_for_hotkey_and_coldkey_on_subnet(
                hotkey, coldkey, netuid, alpha,
            );

            // Step 4: Update the list of hotkeys staking for this coldkey
            let mut staking_hotkeys = StakingHotkeys::<T>::get(coldkey);
            if !staking_hotkeys.contains(hotkey) {
                staking_hotkeys.push(hotkey.clone());
                StakingHotkeys::<T>::insert(coldkey, staking_hotkeys.clone());
            }
        }

        // Step 5. Increase Tao reserves by the fee amount.
        SubnetTAO::<T>::mutate(netuid, |total| {
            *total = total.saturating_add(actual_fee);
        });
        TotalStake::<T>::mutate(|total| {
            *total = total.saturating_add(actual_fee);
        });
        LastColdkeyHotkeyStakeBlock::<T>::insert(coldkey, hotkey, Self::get_current_block_as_u64());

        // Step 6. Deposit and log the staking event.
        Self::deposit_event(Event::StakeAdded(
            coldkey.clone(),
            hotkey.clone(),
            tao_staked,
            actual_alpha,
            netuid,
            actual_fee,
        ));
        log::debug!(
            "StakeAdded( coldkey: {:?}, hotkey:{:?}, tao: {:?}, alpha:{:?}, netuid: {:?}, fee: {:?} )",
            coldkey.clone(),
            hotkey.clone(),
            tao_staked,
            actual_alpha,
            netuid,
            actual_fee
        );

        // Step 7: Return the amount of alpha staked
        actual_alpha
    }

    pub fn get_alpha_share_pool(
        hotkey: <T as frame_system::Config>::AccountId,
        netuid: u16,
    ) -> SharePool<AlphaShareKey<T>, HotkeyAlphaSharePoolDataOperations<T>> {
        let ops = HotkeyAlphaSharePoolDataOperations::new(hotkey, netuid);
        SharePool::<AlphaShareKey<T>, HotkeyAlphaSharePoolDataOperations<T>>::new(ops)
    }

    /// Validate add_stake user input
    ///
    pub fn validate_add_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
        stake_to_be_added: u64,
        max_amount: u64,
        allow_partial: bool,
    ) -> Result<(), Error<T>> {
        // Ensure that the subnet exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Get the minimum balance (and amount) that satisfies the transaction
        let min_amount = DefaultMinStake::<T>::get().saturating_add(DefaultStakingFee::<T>::get());

        // Ensure that the stake_to_be_added is at least the min_amount
        ensure!(stake_to_be_added >= min_amount, Error::<T>::AmountTooLow);

        // Ensure that if partial execution is not allowed, the amount will not cause
        // slippage over desired
        if !allow_partial {
            ensure!(stake_to_be_added <= max_amount, Error::<T>::SlippageTooHigh);
        }

        // Ensure the callers coldkey has enough stake to perform the transaction.
        ensure!(
            Self::can_remove_balance_from_coldkey_account(coldkey, stake_to_be_added),
            Error::<T>::NotEnoughBalanceToStake
        );

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        let expected_alpha = Self::sim_swap_tao_for_alpha(netuid, stake_to_be_added);

        // Ensure that we have adequate liquidity
        ensure!(expected_alpha.is_some(), Error::<T>::InsufficientLiquidity);

        // Ensure hotkey pool is precise enough
        let try_stake_result = Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
            hotkey,
            netuid,
            expected_alpha.unwrap_or(0),
        );
        ensure!(try_stake_result, Error::<T>::InsufficientLiquidity);

        Ok(())
    }

    /// Validate remove_stake user input
    ///
    pub fn validate_remove_stake(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
        netuid: u16,
        alpha_unstaked: u64,
        max_amount: u64,
        allow_partial: bool,
    ) -> Result<(), Error<T>> {
        // Ensure that the subnet exists.
        ensure!(Self::if_subnet_exist(netuid), Error::<T>::SubnetNotExists);

        // Ensure that the stake amount to be removed is above the minimum in tao equivalent.
        if let Some(tao_equivalent) = Self::sim_swap_alpha_for_tao(netuid, alpha_unstaked) {
            ensure!(
                tao_equivalent > DefaultMinStake::<T>::get(),
                Error::<T>::AmountTooLow
            );
        } else {
            return Err(Error::<T>::InsufficientLiquidity);
        };

        // Ensure that if partial execution is not allowed, the amount will not cause
        // slippage over desired
        if !allow_partial {
            ensure!(alpha_unstaked <= max_amount, Error::<T>::SlippageTooHigh);
        }

        // Ensure that the hotkey account exists this is only possible through registration.
        ensure!(
            Self::hotkey_account_exists(hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the hotkey has enough stake to withdraw.
        ensure!(
            Self::has_enough_stake_on_subnet(hotkey, coldkey, netuid, alpha_unstaked),
            Error::<T>::NotEnoughStakeToWithdraw
        );

        Ok(())
    }

    /// Validate stake transition user input
    /// That works for move_stake, transfer_stake, and swap_stake
    ///
    pub fn validate_stake_transition(
        origin_coldkey: &T::AccountId,
        _destination_coldkey: &T::AccountId,
        origin_hotkey: &T::AccountId,
        destination_hotkey: &T::AccountId,
        origin_netuid: u16,
        destination_netuid: u16,
        alpha_amount: u64,
        max_amount: u64,
        maybe_allow_partial: Option<bool>,
        check_transfer_toggle: bool,
    ) -> Result<(), Error<T>> {
        // Ensure that both subnets exist.
        ensure!(
            Self::if_subnet_exist(origin_netuid),
            Error::<T>::SubnetNotExists
        );
        if origin_netuid != destination_netuid {
            ensure!(
                Self::if_subnet_exist(destination_netuid),
                Error::<T>::SubnetNotExists
            );
        }

        ensure!(
            SubtokenEnabled::<T>::get(origin_netuid),
            Error::<T>::SubtokenDisabled
        );

        ensure!(
            SubtokenEnabled::<T>::get(destination_netuid),
            Error::<T>::SubtokenDisabled
        );

        // Ensure that the origin hotkey account exists
        ensure!(
            Self::hotkey_account_exists(origin_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure that the destination hotkey account exists
        ensure!(
            Self::hotkey_account_exists(destination_hotkey),
            Error::<T>::HotKeyAccountNotExists
        );

        // Ensure there is enough stake in the origin subnet.
        let origin_alpha = Self::get_stake_for_hotkey_and_coldkey_on_subnet(
            origin_hotkey,
            origin_coldkey,
            origin_netuid,
        );
        ensure!(
            alpha_amount <= origin_alpha,
            Error::<T>::NotEnoughStakeToWithdraw
        );

        // Ensure that the stake amount to be removed is above the minimum in tao equivalent.
        let tao_equivalent_result = Self::sim_swap_alpha_for_tao(origin_netuid, alpha_amount);
        if let Some(tao_equivalent) = tao_equivalent_result {
            ensure!(
                tao_equivalent > DefaultMinStake::<T>::get(),
                Error::<T>::AmountTooLow
            );
        } else {
            return Err(Error::<T>::InsufficientLiquidity);
        }

        // Ensure that if partial execution is not allowed, the amount will not cause
        // slippage over desired
        if let Some(allow_partial) = maybe_allow_partial {
            if !allow_partial {
                ensure!(alpha_amount <= max_amount, Error::<T>::SlippageTooHigh);
            }
        }

        let expected_alpha =
            Self::sim_swap_tao_for_alpha(destination_netuid, tao_equivalent_result.unwrap_or(0))
                .unwrap_or(0);

        // Ensure that the amount being staked to the new hotkey is precise enough
        let try_stake_result = Self::try_increase_stake_for_hotkey_and_coldkey_on_subnet(
            destination_hotkey,
            destination_netuid,
            expected_alpha,
        );
        ensure!(try_stake_result, Error::<T>::InsufficientLiquidity);

        if check_transfer_toggle {
            // Ensure transfer is toggled.
            ensure!(
                TransferToggle::<T>::get(origin_netuid),
                Error::<T>::TransferDisallowed
            );
            ensure!(
                TransferToggle::<T>::get(destination_netuid),
                Error::<T>::TransferDisallowed
            );
        }

        Ok(())
    }

    pub(crate) fn calculate_staking_fee(
        origin: Option<(&T::AccountId, u16)>,
        _origin_coldkey: &T::AccountId,
        destination: Option<(&T::AccountId, u16)>,
        _destination_coldkey: &T::AccountId,
        alpha_estimate: U96F32,
    ) -> u64 {
        match origin {
            // If origin is defined, we are removing/moving stake
            Some((origin_hotkey, origin_netuid)) => {
                if let Some((_destination_hotkey, destination_netuid)) = destination {
                    // This is a stake move/swap/transfer
                    if destination_netuid == origin_netuid {
                        // If destination is on the same subnet, use the default fee
                        return DefaultStakingFee::<T>::get();
                    }
                }

                if origin_netuid == Self::get_root_netuid()
                    || SubnetMechanism::<T>::get(origin_netuid) == 0
                {
                    // If the origin netuid is root, or the subnet mechanism is 0, use the default fee
                    DefaultStakingFee::<T>::get()
                } else {
                    // Otherwise, calculate the fee based on the alpha estimate
                    // Here we are using TotalHotkeyAlphaLastEpoch, which is exactly the value that
                    // was used to calculate AlphaDividendsPerSubnet
                    let tao_estimate = U96F32::saturating_from_num(
                        Self::sim_swap_alpha_for_tao(
                            origin_netuid,
                            alpha_estimate.saturating_to_num::<u64>(),
                        )
                        .unwrap_or(0),
                    );
                    let mut fee = tao_estimate
                        .saturating_mul(
                            U96F32::saturating_from_num(AlphaDividendsPerSubnet::<T>::get(
                                origin_netuid,
                                &origin_hotkey,
                            ))
                            .safe_div(U96F32::saturating_from_num(
                                TotalHotkeyAlphaLastEpoch::<T>::get(&origin_hotkey, origin_netuid),
                            )),
                        )
                        .saturating_to_num::<u64>();

                    // 0.005% per epoch matches to 44% annual in compound interest. Do not allow the fee
                    // to be lower than that. (1.00005^(365*20) ~= 1.44)
                    let apr_20_percent = U96F32::saturating_from_num(0.00005);
                    fee = fee.max(
                        tao_estimate
                            .saturating_mul(apr_20_percent)
                            .saturating_to_num::<u64>(),
                    );

                    // We should at least get DefaultStakingFee anyway
                    fee.max(DefaultStakingFee::<T>::get())
                }
            }
            // If origin is not defined, we are adding stake; use default fee
            None => DefaultStakingFee::<T>::get(),
        }
    }

    // Process staking job for on_finalize() hook.
    pub(crate) fn do_on_finalize(current_block_number: BlockNumberFor<T>) {
        // We delay job execution
        const DELAY_IN_BLOCKS: u32 = 1u32;
        let actual_block_with_delay = current_block_number.saturating_sub(DELAY_IN_BLOCKS.into());

        let stake_jobs = StakeJobs::<T>::drain_prefix(actual_block_with_delay).collect::<Vec<_>>();

        // Sort jobs by job type
        let mut add_stake = vec![];
        let mut remove_stake = vec![];
        let mut add_stake_limit = vec![];
        let mut remove_stake_limit = vec![];
        let mut unstake_all = vec![];
        let mut unstake_all_aplha = vec![];

        for (_, job) in stake_jobs.into_iter() {
            match &job {
                StakeJob::AddStake { .. } => add_stake.push(job),
                StakeJob::RemoveStake { .. } => remove_stake.push(job),
                StakeJob::AddStakeLimit { .. } => add_stake_limit.push(job),
                StakeJob::RemoveStakeLimit { .. } => remove_stake_limit.push(job),
                StakeJob::UnstakeAll { .. } => unstake_all.push(job),
                StakeJob::UnstakeAllAlpha { .. } => unstake_all_aplha.push(job),
            }
        }
        // Reorder jobs based on the previous block hash
        let previous_block_hash = <frame_system::Pallet<T>>::parent_hash();
        let hash_bytes = previous_block_hash.as_ref();
        let first_byte = hash_bytes.first().expect("hash operation is infallible");
        // Extract the first bit
        let altered_order = (first_byte & 0b10000000) != 0;

        // Ascending sort by coldkey
        remove_stake_limit.sort_by(|a, b| match (a, b) {
            (
                StakeJob::RemoveStakeLimit { coldkey: a_key, .. },
                StakeJob::RemoveStakeLimit { coldkey: b_key, .. },
            ) => {
                let direct_order = a_key.cmp(b_key); // ascending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        remove_stake.sort_by(|a, b| match (a, b) {
            (
                StakeJob::RemoveStake { coldkey: a_key, .. },
                StakeJob::RemoveStake { coldkey: b_key, .. },
            ) => {
                let direct_order = a_key.cmp(b_key); // ascending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        unstake_all.sort_by(|a, b| match (a, b) {
            (
                StakeJob::UnstakeAll { coldkey: a_key, .. },
                StakeJob::UnstakeAll { coldkey: b_key, .. },
            ) => {
                let direct_order = a_key.cmp(b_key); // ascending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        unstake_all_aplha.sort_by(|a, b| match (a, b) {
            (
                StakeJob::UnstakeAllAlpha { coldkey: a_key, .. },
                StakeJob::UnstakeAllAlpha { coldkey: b_key, .. },
            ) => {
                let direct_order = a_key.cmp(b_key); // ascending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        // Descending sort by coldkey
        add_stake_limit.sort_by(|a, b| match (a, b) {
            (
                StakeJob::AddStakeLimit { coldkey: a_key, .. },
                StakeJob::AddStakeLimit { coldkey: b_key, .. },
            ) => {
                let direct_order = b_key.cmp(a_key); // descending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        add_stake.sort_by(|a, b| match (a, b) {
            (
                StakeJob::AddStake { coldkey: a_key, .. },
                StakeJob::AddStake { coldkey: b_key, .. },
            ) => {
                let direct_order = b_key.cmp(a_key); // descending

                if altered_order {
                    direct_order.reverse()
                } else {
                    direct_order
                }
            }
            _ => sp_std::cmp::Ordering::Equal, // unreachable
        });

        // direct job order
        let mut job_batches = vec![
            remove_stake_limit,
            remove_stake,
            unstake_all,
            unstake_all_aplha,
            add_stake_limit,
            add_stake,
        ];
        if altered_order {
            job_batches.reverse();
        }

        for jobs in job_batches.into_iter() {
            for job in jobs.into_iter() {
                match job {
                    StakeJob::RemoveStakeLimit {
                        hotkey,
                        coldkey,
                        netuid,
                        alpha_unstaked,
                        limit_price,
                        allow_partial,
                    } => {
                        let result = Self::do_remove_stake_limit(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                            netuid,
                            alpha_unstaked,
                            limit_price,
                            allow_partial,
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to remove aggregated limited stake: {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                                limit_price,
                                allow_partial,
                                err
                            );
                            Self::deposit_event(Event::FailedToRemoveAggregatedLimitedStake(
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                                limit_price,
                                allow_partial,
                            ));
                        } else {
                            Self::deposit_event(Event::AggregatedLimitedStakeRemoved(
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                                limit_price,
                                allow_partial,
                            ));
                        }
                    }
                    StakeJob::RemoveStake {
                        coldkey,
                        hotkey,
                        netuid,
                        alpha_unstaked,
                    } => {
                        let result = Self::do_remove_stake(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                            netuid,
                            alpha_unstaked,
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to remove aggregated stake: {:?}, {:?}, {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                                err
                            );
                            Self::deposit_event(Event::FailedToRemoveAggregatedStake(
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                            ));
                        } else {
                            Self::deposit_event(Event::AggregatedStakeRemoved(
                                coldkey,
                                hotkey,
                                netuid,
                                alpha_unstaked,
                            ));
                        }
                    }
                    StakeJob::UnstakeAll { hotkey, coldkey } => {
                        let result = Self::do_unstake_all(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to unstake all: {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                err
                            );
                            Self::deposit_event(Event::AggregatedUnstakeAllFailed(coldkey, hotkey));
                        } else {
                            Self::deposit_event(Event::AggregatedUnstakeAllSucceeded(
                                coldkey, hotkey,
                            ));
                        }
                    }
                    StakeJob::UnstakeAllAlpha { hotkey, coldkey } => {
                        let result = Self::do_unstake_all_alpha(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to unstake all alpha: {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                err
                            );
                            Self::deposit_event(Event::AggregatedUnstakeAllAlphaFailed(
                                coldkey, hotkey,
                            ));
                        } else {
                            Self::deposit_event(Event::AggregatedUnstakeAllAlphaSucceeded(
                                coldkey, hotkey,
                            ));
                        }
                    }
                    StakeJob::AddStakeLimit {
                        hotkey,
                        coldkey,
                        netuid,
                        stake_to_be_added,
                        limit_price,
                        allow_partial,
                    } => {
                        let result = Self::do_add_stake_limit(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                            netuid,
                            stake_to_be_added,
                            limit_price,
                            allow_partial,
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to add aggregated limited stake: {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                                limit_price,
                                allow_partial,
                                err
                            );
                            Self::deposit_event(Event::FailedToAddAggregatedLimitedStake(
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                                limit_price,
                                allow_partial,
                            ));
                        } else {
                            Self::deposit_event(Event::AggregatedLimitedStakeAdded(
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                                limit_price,
                                allow_partial,
                            ));
                        }
                    }
                    StakeJob::AddStake {
                        hotkey,
                        coldkey,
                        netuid,
                        stake_to_be_added,
                    } => {
                        let result = Self::do_add_stake(
                            dispatch::RawOrigin::Signed(coldkey.clone()).into(),
                            hotkey.clone(),
                            netuid,
                            stake_to_be_added,
                        );

                        if let Err(err) = result {
                            log::debug!(
                                "Failed to add aggregated stake: {:?}, {:?}, {:?}, {:?}, {:?}",
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                                err
                            );
                            Self::deposit_event(Event::FailedToAddAggregatedStake(
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                            ));
                        } else {
                            Self::deposit_event(Event::AggregatedStakeAdded(
                                coldkey,
                                hotkey,
                                netuid,
                                stake_to_be_added,
                            ));
                        }
                    }
                }
            }
        }
    }
}

///////////////////////////////////////////
// Alpha share pool chain data layer

#[derive(Debug)]
pub struct HotkeyAlphaSharePoolDataOperations<T: frame_system::Config> {
    netuid: u16,
    hotkey: <T as frame_system::Config>::AccountId,
    _marker: sp_std::marker::PhantomData<T>,
}

impl<T: Config> HotkeyAlphaSharePoolDataOperations<T> {
    fn new(hotkey: <T as frame_system::Config>::AccountId, netuid: u16) -> Self {
        HotkeyAlphaSharePoolDataOperations {
            netuid,
            hotkey,
            _marker: sp_std::marker::PhantomData,
        }
    }
}

// Alpha share key is coldkey because the HotkeyAlphaSharePoolDataOperations struct already has hotkey and netuid
type AlphaShareKey<T> = <T as frame_system::Config>::AccountId;

impl<T: Config> SharePoolDataOperations<AlphaShareKey<T>>
    for HotkeyAlphaSharePoolDataOperations<T>
{
    fn get_shared_value(&self) -> U64F64 {
        U64F64::saturating_from_num(crate::TotalHotkeyAlpha::<T>::get(&self.hotkey, self.netuid))
    }

    fn get_share(&self, key: &AlphaShareKey<T>) -> U64F64 {
        crate::Alpha::<T>::get((&(self.hotkey), key, self.netuid))
    }

    fn try_get_share(&self, key: &AlphaShareKey<T>) -> Result<U64F64, ()> {
        crate::Alpha::<T>::try_get((&(self.hotkey), key, self.netuid))
    }

    fn get_denominator(&self) -> U64F64 {
        crate::TotalHotkeyShares::<T>::get(&(self.hotkey), self.netuid)
    }

    fn set_shared_value(&mut self, value: U64F64) {
        if value != 0 {
            crate::TotalHotkeyAlpha::<T>::insert(
                &(self.hotkey),
                self.netuid,
                value.saturating_to_num::<u64>(),
            );
        } else {
            crate::TotalHotkeyAlpha::<T>::remove(&(self.hotkey), self.netuid);
        }
    }

    fn set_share(&mut self, key: &AlphaShareKey<T>, share: U64F64) {
        if share != 0 {
            crate::Alpha::<T>::insert((&self.hotkey, key, self.netuid), share);
        } else {
            crate::Alpha::<T>::remove((&self.hotkey, key, self.netuid));
        }
    }

    fn set_denominator(&mut self, update: U64F64) {
        if update != 0 {
            crate::TotalHotkeyShares::<T>::insert(&self.hotkey, self.netuid, update);
        } else {
            crate::TotalHotkeyShares::<T>::remove(&self.hotkey, self.netuid);
        }
    }
}
