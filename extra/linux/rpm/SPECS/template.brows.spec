Name:           brows
Version:        €Version€
Release:        1
Summary:        Context menu displaying a list of apps when clicking on a web link
License:        MIT AND Apache-2.0
Requires:       fontconfig, freetype, libX11, libxcb, libxkbcommon, libxkbcommon-x11, vulkan-loader, wayland

%define _rpmfilename %%{NAME}.%%{ARCH}.rpm
%define _bindir /usr/bin
%define _datadir /usr/share
%define _arch €Architecture€
%define _tree %{_topdir}/tree

# https://groups.google.com/g/linux.redhat.rpm/c/TKz6JZHK0ck
%define __os_install_post %{nil}

# also see https://docs.fedoraproject.org/en-US/packaging-guidelines/#_tags_and_sections
# also see https://jfearn.fedorapeople.org/fdocs/en-US/Fedora_Draft_Documentation/0.1/
#          html/Packagers_Guide/chap-Packagers_Guide-Spec_File_Reference-Preamble.html

%description
brows is an intuitive context menu that pops up
when you press a link in an app other than a web browser.

%prep
# we have no source, so nothing here

%install
# Don't execute strip command on binary, because that would work only
# on binaries with same architecture as the runner
# but we are running it in x86_64 on github ci runner
#export DONT_STRIP=1

ls -altrh %{_tree}
ls -altrh %{_tree}/usr/bin
ls -altrh %{_tree}/usr/share

mkdir -p %{buildroot}%{_bindir}
mkdir -p %{buildroot}%{_datadir}/me.tarik02.brows/bin
mkdir -p %{buildroot}%{_datadir}/me.tarik02.brows/resources/i18n/en-US
mkdir -p %{buildroot}%{_datadir}/me.tarik02.brows/resources/icons/512x512
mkdir -p %{buildroot}%{_datadir}/me.tarik02.brows/resources/repository
mkdir -p %{buildroot}%{_datadir}/applications/xfce4/helpers
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/512x512/apps
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/256x256/apps
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/128x128/apps
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/64x64/apps
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/32x32/apps
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/16x16/apps

# Copy all files directly to buildroot
# + mkdir -p /.../BUILDROOT/brows-0.3.9-1.arm64/usr/share/applications
# -a is same as -dR --preserve=all
# For us it's important that it:
#  - preserves symlinks
#  - copies recursively
#cp -a %{_tree}/ %{buildroot}/

cp -a %{_tree}%{_datadir}/applications/me.tarik02.brows.desktop %{buildroot}%{_datadir}/applications/me.tarik02.brows.desktop
cp -a %{_tree}%{_datadir}/applications/xfce4/helpers/me.tarik02.brows.desktop %{buildroot}%{_datadir}/applications/xfce4/helpers/me.tarik02.brows.desktop
mkdir -p %{buildroot}%{_datadir}/dbus-1/services
cp -a %{_tree}%{_datadir}/dbus-1/services/me.tarik02.brows.service %{buildroot}%{_datadir}/dbus-1/services/me.tarik02.brows.service

for size in 16 32 64 128 256 512; do
    cp -a %{_tree}%{_datadir}/icons/hicolor/${size}x${size}/apps/me.tarik02.brows.png %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/me.tarik02.brows.png
done

cp -a %{_tree}%{_datadir}/me.tarik02.brows/bin/brows %{buildroot}%{_datadir}/me.tarik02.brows/bin/brows
cp -a %{_tree}%{_datadir}/me.tarik02.brows/resources/i18n/en-US/builtin.ftl %{buildroot}%{_datadir}/me.tarik02.brows/resources/i18n/en-US/builtin.ftl
cp -a %{_tree}%{_datadir}/me.tarik02.brows/resources/icons/512x512/me.tarik02.brows.png %{buildroot}%{_datadir}/me.tarik02.brows/resources/icons/512x512/me.tarik02.brows.png
cp -a %{_tree}%{_datadir}/me.tarik02.brows/resources/repository/application-repository.toml %{buildroot}%{_datadir}/me.tarik02.brows/resources/repository/application-repository.toml

cp -a %{_tree}%{_bindir}/brows %{buildroot}%{_bindir}/brows

%files
# set default user and group as root
%defattr(-,root,root,-)

# _bindir: /usr/bin
# _datadir: /usr/share

%{_bindir}/brows
%{_datadir}/applications/me.tarik02.brows.desktop
%{_datadir}/applications/xfce4/helpers/me.tarik02.brows.desktop
%{_datadir}/dbus-1/services/me.tarik02.brows.service
%{_datadir}/icons/hicolor/*/apps/me.tarik02.brows.png
%{_datadir}/me.tarik02.brows/bin/brows
%{_datadir}/me.tarik02.brows/resources/i18n/en-US/builtin.ftl
%{_datadir}/me.tarik02.brows/resources/icons/512x512/me.tarik02.brows.png
%{_datadir}/me.tarik02.brows/resources/repository/application-repository.toml

%changelog
# let's skip this for now
