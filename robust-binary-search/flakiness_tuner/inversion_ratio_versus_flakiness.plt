set terminal png size 800,600
set out 'inversion_ratio_versus_flakiness.png'
unset key
set xlabel 'Flakiness'
set ylabel 'Inversion ratio'
plot 'flakiness_benchmark.dat' u 1:($2/$3)
