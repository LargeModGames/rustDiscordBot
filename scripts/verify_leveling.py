#!/usr/bin/env python3
"""
Verification script for the new leveling formula.
Calculates XP requirements for key levels and compares with the new formula.
"""

def xp_for_level_old(level):
    """Old formula: 100 * (level - 1)^1.5"""
    if level <= 1:
        return 0
    return int(100 * ((level - 1) ** 1.5))

def xp_for_level_new(level):
    """New formula: 60 * (level - 1)^1.35"""
    if level <= 1:
        return 0
    return int(60 * ((level - 1) ** 1.35))

def main():
    print("=" * 80)
    print("LEVELING SYSTEM REBALANCE VERIFICATION")
    print("=" * 80)
    print()
    
    print("Formula Change:")
    print("  OLD: 100 * (level - 1)^1.5")
    print("  NEW: 60 * (level - 1)^1.35")
    print()
    
    # Test key levels
    test_levels = [1, 5, 10, 15, 25, 45, 50, 75, 100]
    
    print(f"{'Level':<8} {'Old XP':<12} {'New XP':<12} {'Reduction':<12} {'% Easier'}")
    print("-" * 80)
    
    for level in test_levels:
        old_xp = xp_for_level_old(level)
        new_xp = xp_for_level_new(level)
        reduction = old_xp - new_xp
        percent = (reduction / old_xp * 100) if old_xp > 0 else 0
        
        print(f"{level:<8} {old_xp:<12,} {new_xp:<12,} {reduction:<12,} {percent:>6.1f}%")
    
    print()
    print("=" * 80)
    print("TIME TO LEVEL 100 ESTIMATION")
    print("=" * 80)
    print()
    
    # Based on user's actual data: 3 months to level 45
    actual_xp_per_day = xp_for_level_old(45) / 90  # ~324 XP/day
    
    print(f"User's actual rate: {actual_xp_per_day:.0f} XP/day (based on 3 months to level 45)")
    print()
    
    # Calculate time to level 100 with both formulas
    old_total_xp = xp_for_level_old(100)
    new_total_xp = xp_for_level_new(100)
    
    old_days = old_total_xp / actual_xp_per_day
    new_days = new_total_xp / actual_xp_per_day
    
    print(f"Old formula:")
    print(f"  Total XP for level 100: {old_total_xp:,}")
    print(f"  Days needed: {old_days:.0f} days = {old_days/30:.1f} months")
    print()
    
    print(f"New formula:")
    print(f"  Total XP for level 100: {new_total_xp:,}")
    print(f"  Days needed: {new_days:.0f} days = {new_days/30:.1f} months")
    print()
    
    print(f"Improvement: {old_days - new_days:.0f} days faster ({(1 - new_days/old_days)*100:.0f}% reduction)")
    print()
    
    # Progress from level 45 to 100
    print("=" * 80)
    print("PROGRESS FROM LEVEL 45 → 100")
    print("=" * 80)
    print()
    
    old_xp_45 = xp_for_level_old(45)
    new_xp_45 = xp_for_level_new(45)
    
    old_needed = old_total_xp - old_xp_45
    new_needed = new_total_xp - new_xp_45
    
    old_days_remaining = old_needed / actual_xp_per_day
    new_days_remaining = new_needed / actual_xp_per_day
    
    print(f"Old formula: {old_needed:,} XP = {old_days_remaining:.0f} days ({old_days_remaining/30:.1f} months)")
    print(f"New formula: {new_needed:,} XP = {new_days_remaining:.0f} days ({new_days_remaining/30:.1f} months)")
    print()
    print(f"✅ New system is {old_days_remaining - new_days_remaining:.0f} days faster!")
    print()

if __name__ == "__main__":
    main()
