**FREE
ctl-opt dftactgrp(*no);
dcl-s count int(10) inz(0);
dcl-s name char(10) inz('DESK');
dcl-ds asset qualified;
  id int(10);
  tag char(8);
end-ds;
count = count + 1;
asset.id = count;
// the tag
asset.tag = 'A' + %char(count);
name = 'CHAIR';
*inlr = *on;
