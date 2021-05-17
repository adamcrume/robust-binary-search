# To generate the plot, run the tuner:
#   cargo run --bin flakiness_tuner --features flakiness_tuner -- $PWD/flakiness_benchmark.dat
# and then:
#   gnuplot recovered_flakiness.plt

set terminal png size 800,600
set out 'recovered_flakiness.png'
set key right bottom
set xlabel 'True flakiness'
set ylabel 'Recovered flakiness'
f(x) = a*x**2 + b*x
fit f(x) 'flakiness_benchmark.dat' u ($2/$3):1 via a, b
plot 'flakiness_benchmark.dat' u (f($2/$3)):1 t sprintf("Recovered flakiness: %.4f ρ² + %.4f ρ", a, b), \
  x t 'True flakiness'
