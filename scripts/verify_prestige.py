#!/usr/bin/env python3
"""
Prestige System Calculations and Visualization
Helps understand the impact of prestige tiers on progression
"""

def xp_for_level(level):
    """Current rebalanced formula: 60 * (level - 1)^1.35"""
    if level <= 1:
        return 0
    return int(60 * ((level - 1) ** 1.35))

def get_prestige_tier(prestige_level):
    """Return tier information for a given prestige level"""
    tiers = {
        0: ("Novice", 1.00, 0, "-"),
        1: ("Bronze", 1.05, 0, "🥉"),
        2: ("Silver", 1.10, 0, "🥈"),
        3: ("Gold", 1.15, 50, "🥇"),
        4: ("Platinum", 1.20, 100, "💎"),
        5: ("Diamond", 1.25, 150, "💠"),
        6: ("Master", 1.30, 200, "⭐"),
        7: ("Grandmaster", 1.35, 250, "🌟"),
        8: ("Legend", 1.40, 300, "✨"),
        9: ("Mythic", 1.45, 350, "🔥"),
    }
    
    if prestige_level >= 10:
        return ("Transcendent", 1.50, 400, "👑")
    return tiers.get(prestige_level, tiers[0])

def calculate_time_to_level_50(prestige_level, base_xp_per_day=324):
    """Calculate days to reach level 50 with prestige bonuses"""
    tier_name, multiplier, daily_bonus, badge = get_prestige_tier(prestige_level)
    
    # Total XP needed to reach level 50
    total_xp_needed = xp_for_level(50)
    
    # Effective XP per day with multiplier
    effective_xp_per_day = base_xp_per_day * multiplier + daily_bonus
    
    days = total_xp_needed / effective_xp_per_day
    
    return {
        'tier_name': tier_name,
        'badge': badge,
        'multiplier': multiplier,
        'daily_bonus': daily_bonus,
        'effective_xp_per_day': effective_xp_per_day,
        'total_xp_needed': total_xp_needed,
        'days': days,
        'weeks': days / 7,
        'months': days / 30
    }

def main():
    print("=" * 80)
    print("PRESTIGE SYSTEM ANALYSIS")
    print("=" * 80)
    print()
    
    # Based on user's actual progression: 3 months to level 45
    base_xp_per_day = xp_for_level(45) / 90  # ~324 XP/day
    print(f"Baseline: {base_xp_per_day:.0f} XP/day (based on 3 months to level 45)")
    print(f"XP needed for level 50: {xp_for_level(50):,}")
    print()
    
    print("=" * 80)
    print("TIME TO REACH LEVEL 50 BY PRESTIGE TIER")
    print("=" * 80)
    print()
    
    header = f"{'Tier':<6} {'Badge':<6} {'Name':<15} {'Mult':<7} {'Daily':<8} {'Eff XP/day':<12} {'Days':<7} {'Weeks':<7} {'Months'}"
    print(header)
    print("-" * 80)
    
    for prestige in range(11):
        result = calculate_time_to_level_50(prestige, base_xp_per_day)
        print(f"{prestige:<6} {result['badge']:<6} {result['tier_name']:<15} "
              f"{result['multiplier']:<7.2f} {result['daily_bonus']:<8} "
              f"{result['effective_xp_per_day']:<12.0f} {result['days']:<7.0f} "
              f"{result['weeks']:<7.1f} {result['months']:.1f}")
    
    print()
    print("=" * 80)
    print("PRESTIGE PROGRESSION TIMELINE")
    print("=" * 80)
    print()
    
    print("Assuming consistent daily activity:")
    print()
    
    cumulative_days = 0
    for i in range(6):  # Show first 5 prestiges
        result = calculate_time_to_level_50(i, base_xp_per_day)
        cumulative_days += result['days']
        
        print(f"Prestige {i} → {i+1}:")
        print(f"  {result['badge']} {result['tier_name']} (Mult: {result['multiplier']}x, Daily: +{result['daily_bonus']} XP)")
        print(f"  Time: {result['days']:.0f} days ({result['weeks']:.1f} weeks)")
        print(f"  Cumulative: {cumulative_days:.0f} days ({cumulative_days/30:.1f} months)")
        print()
    
    print("=" * 80)
    print("COMPARISON: PRESTIGE 0 vs PRESTIGE 10")
    print("=" * 80)
    print()
    
    p0 = calculate_time_to_level_50(0, base_xp_per_day)
    p10 = calculate_time_to_level_50(10, base_xp_per_day)
    
    time_saved = p0['days'] - p10['days']
    percent_faster = (time_saved / p0['days']) * 100
    
    print(f"Prestige 0 (Novice):      {p0['days']:.0f} days ({p0['months']:.1f} months)")
    print(f"Prestige 10 (Transcendent): {p10['days']:.0f} days ({p10['months']:.1f} months)")
    print()
    print(f"⚡ Prestige 10 is {time_saved:.0f} days faster ({percent_faster:.0f}% speed increase)!")
    print()
    
    print("=" * 80)
    print("XP MULTIPLIER IMPACT ON DAILY GAINS")
    print("=" * 80)
    print()
    
    print(f"Base XP gain example: 100 XP from a message")
    print()
    print(f"{'Prestige':<10} {'Tier':<15} {'XP Gained':<12} {'Increase'}")
    print("-" * 80)
    
    base_gain = 100
    for prestige in [0, 1, 3, 5, 7, 10]:
        tier_name, multiplier, daily_bonus, badge = get_prestige_tier(prestige)
        gained = base_gain * multiplier
        increase = gained - base_gain
        print(f"{prestige:<10} {tier_name:<15} {gained:<12.0f} +{increase:.0f} XP ({(multiplier-1)*100:.0f}%)")
    
    print()

if __name__ == "__main__":
    main()
