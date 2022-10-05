#set terminal png size 800,600
set terminal wxt
set out 'flakiness_split_tuner.png'
#unset key
#set dgrid3d 50,50
#set grid
set hidden3d
set ticslevel 0
f(x,y) = a*x + b*x*x + c*y + d*y*y + e*x*y + f + g*x*x*x + h*x*x*y + i*x*y*y + j*y*y*y
fit f(x,y) 'flakiness_split_tuner.dat' u 3:4:1 via a,b,c,d,e,f,g,h,i,j

splot 'flakiness_split_tuner.dat' u 3:4:1, \
      f(x,y)

#splot 'flakiness_split_tuner.dat' u 1:2:3 t 'low' with lines, \
#      'flakiness_split_tuner.dat' u 1:2:4 t 'high' with lines
