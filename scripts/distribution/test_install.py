#!/usr/bin/env python3
"""Verify the Linux user installer in temporary paths without touching live installations."""
import json, pathlib, subprocess, tempfile, unittest
INSTALLER=pathlib.Path(__file__).with_name('install-linux.py')
class InstallerTests(unittest.TestCase):
    def test_install_upgrade_refusal_and_uninstall_preserve_unrelated_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);package=root/'package';prefix=root/'application';bin_dir=root/'bin';desktop_dir=root/'desktop'
            for relative,text in [('bin/mpai-desktop','native host'),('bin/mpai-desktop_services','runtime'),('resources/index.html','frontend')]:
                path=package/relative;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
            args=['python3',str(INSTALLER),'--package',str(package),'--prefix',str(prefix),'--bin-dir',str(bin_dir),'--desktop-dir',str(desktop_dir)]
            subprocess.run(args,check=True,capture_output=True)
            self.assertEqual((bin_dir/'mpai').resolve(),prefix/'bin/mpai-desktop_services')
            self.assertIn('X-Mpai-Install=',(desktop_dir/'dev.mpai.desktop.desktop').read_text())
            (root/'sessions.json').write_text('user history')
            (package/'bin/mpai-desktop_services').write_text('new runtime')
            subprocess.run(args,check=True,capture_output=True)
            self.assertEqual((bin_dir/'mpai').read_text(),'new runtime')
            subprocess.run(args+['--uninstall'],check=True,capture_output=True)
            self.assertFalse(prefix.exists());self.assertFalse((bin_dir/'mpai').is_symlink())
            self.assertEqual((root/'sessions.json').read_text(),'user history')
            prefix.mkdir();(prefix/'unknown').write_text('unmanaged')
            refused=subprocess.run(args,capture_output=True)
            self.assertNotEqual(refused.returncode,0)
            self.assertEqual((prefix/'unknown').read_text(),'unmanaged')
if __name__=='__main__':unittest.main()
