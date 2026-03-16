taskkill /f /im explorer.exe
attrib -h -s -r "%userprofile%\AppData\Local\IconCache.db"
del /f /q "%userprofile%\AppData\Local\IconCache.db"
attrib -h -s -r "%userprofile%\AppData\Local\Microsoft\Windows\Explorer\iconcache_*.db"
del /f /q "%userprofile%\AppData\Local\Microsoft\Windows\Explorer\iconcache_*.db"
start explorer