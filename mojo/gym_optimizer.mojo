from std.math import log, log10, pow, round
from std.sys import argv

def round_to(x: Float64, dp: Int) -> Float64:
    var p = Float64(10 ** dp)
    return round(x * p) / p

def soft_cap_stat(stat: Float64) -> Float64:
    var cap: Float64 = 50000000.0
    var factor: Float64 = 8.77635
    if stat <= cap:
        return stat
    return cap + (stat - cap) / (factor * log10(stat))

def expected_gain_per_train(
    stat_name: String,
    stat_val: Float64,
    happy: Float64,
    energy_per_train: Float64,
    gym_dots: Float64,
    perk_mult: Float64
) -> Float64:
    var a: Float64 = 1600.0
    var b: Float64 = 1700.0
    if stat_name == "strength":
        a = 1600.0
        b = 1700.0
    elif stat_name == "speed":
        a = 1600.0
        b = 2000.0
    elif stat_name == "dexterity":
        a = 1800.0
        b = 1500.0
    elif stat_name == "defense":
        a = 2100.0
        b = -600.0

    var h = happy
    if h < 0.0:
        h = 0.0

    var seff = soft_cap_stat(stat_val)
    var log_term = round_to(log(1.0 + h / 250.0), 4)
    var happy_mult = round_to(1.0 + 0.07 * log_term, 4)
    var happy_power = 8.0 * pow(h, 1.05)
    var happy_taper = (1.0 - pow(h / 99999.0, 2.0)) * a
    var inner = seff * happy_mult + happy_power + happy_taper + b

    return (inner / 200000.0) * gym_dots * energy_per_train * perk_mult

def simulate_session(
    stat_name: String,
    start_stat: Float64,
    start_happy: Float64,
    energy_available: Float64,
    energy_per_train: Float64,
    gym_dots: Float64,
    perk_mult: Float64,
    happy_loss_per_train: Float64,
    min_happy: Float64
) -> Tuple[Int, Float64, Float64, Float64]:
    var stat = start_stat
    var happy = start_happy
    var energy = energy_available
    var trains: Int = 0
    var total_gain: Float64 = 0.0

    while energy >= energy_per_train:
        var gain = expected_gain_per_train(
            stat_name, stat, happy, energy_per_train, gym_dots, perk_mult
        )
        stat += gain
        total_gain += gain
        happy = happy - happy_loss_per_train
        if happy < min_happy:
            happy = min_happy
        energy -= energy_per_train
        trains += 1

    return (trains, total_gain, stat, happy)

def main() raises:
    if len(argv()) < 8:
        print("Usage: gym_optimizer.mojo <stat> <start_stat> <start_happy> <energy_total> <energy_per_train> <gym_dots> <perk_mult>")
        print("Demo run:")
        var res = simulate_session("strength", 1000000.0, 5000.0, 1000.0, 5.0, 7.3, 1.0, 2.5, 0.0)
        print("Trains:", res[0])
        print("Total Gain:", res[1])
        print("End Stat:", res[2])
        print("End Happy:", res[3])
        return

    var stat_name = argv()[1]
    var start_stat = Float64(argv()[2])
    var start_happy = Float64(argv()[3])
    var energy_available = Float64(argv()[4])
    var energy_per_train = Float64(argv()[5])
    var gym_dots = Float64(argv()[6])
    var perk_mult = Float64(argv()[7])

    var loss_per_train = 0.5 * energy_per_train
    var res = simulate_session(
        stat_name, start_stat, start_happy, energy_available, energy_per_train, gym_dots, perk_mult, loss_per_train, 0.0
    )

    print("{")
    print("  \"stat\": \"" + stat_name + "\",")
    print("  \"trains\": " + String(res[0]) + ",")
    print("  \"total_gain\": " + String(res[1]) + ",")
    print("  \"end_stat\": " + String(res[2]) + ",")
    print("  \"end_happy\": " + String(res[3]) + ",")
    print("  \"energy_spent\": " + String(Float64(res[0]) * energy_per_train))
    print("}")
