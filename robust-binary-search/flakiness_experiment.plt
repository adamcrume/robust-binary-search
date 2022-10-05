set terminal png size 800,600
set out 'flakiness_experiment.png'
unset key
set dgrid3d 30,30
#set grid
set hidden3d
set ticslevel 0
splot 'flakiness_experiment.dat' u 1:2:3 t 'unsplit' with lines, \
      'flakiness_experiment.dat' u 1:2:4 t 'split' with lines
