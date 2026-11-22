; plotter-studio 0.1.0
; job job-017a3c5b7a0e3b5a
; machine generic.cutter-small@1.0.0  mat 1/1 12x12in standard
; T1 pen.fineliner-black-0.4 holder A
; T2 generic.drag-blade holder B
; estimate 8s  tool-changes 0
G21
G90
M0 (Load pen: Fine liner, black 0.4 into Clamp A; Load blade: Fine-point blade [45°] into Clamp B)
T1 M6
G0 X60.000 Y10.000
M3 S120
G1 X100.000 Y10.000 F6000
G1 X100.000 Y40.000
G1 X60.000 Y40.000
G1 X60.000 Y10.000
M5
T2 M6
G0 X30.250 Y10.000
M3 S350
G1 X50.250 Y10.000
G3 X50.000 Y10.250 I-0.250 J0.000
G1 X50.000 Y40.250
G3 X49.750 Y40.000 I0.000 J-0.250
G1 X9.750 Y40.000
G3 X10.000 Y39.750 I0.250 J0.000
G1 X10.000 Y9.750
G3 X10.250 Y10.000 I0.000 J0.250
G1 X31.250 Y10.000
M5
G0 X0.000 Y0.000
M0 (Unload mat)
M2
