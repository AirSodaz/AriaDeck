Name:           ariadeck
Version:        %{version}
Release:        1%{?dist}
Summary:        Native desktop client for aria2
License:        MIT
URL:            https://github.com/AirSodaz/AriaDeck
AutoReqProv:    yes

%description
AriaDeck is a native Rust desktop client that controls aria2 over JSON-RPC.

%prep

%build

%install
mkdir -p %{buildroot}%{_bindir}
mkdir -p %{buildroot}%{_datadir}/applications
mkdir -p %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
mkdir -p %{buildroot}%{_docdir}/ariadeck

install -m 0755 %{_sourcedir}/ariadeck-desktop %{buildroot}%{_bindir}/ariadeck-desktop
install -m 0644 %{_sourcedir}/ariadeck.desktop %{buildroot}%{_datadir}/applications/ariadeck.desktop
install -m 0644 %{_sourcedir}/ariadeck.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/ariadeck.svg
install -m 0644 %{_sourcedir}/LICENSE %{buildroot}%{_docdir}/ariadeck/LICENSE
install -m 0644 %{_sourcedir}/THIRD_PARTY_NOTICES.md %{buildroot}%{_docdir}/ariadeck/THIRD_PARTY_NOTICES.md

%files
%{_bindir}/ariadeck-desktop
%{_datadir}/applications/ariadeck.desktop
%{_datadir}/icons/hicolor/scalable/apps/ariadeck.svg
%{_docdir}/ariadeck/LICENSE
%{_docdir}/ariadeck/THIRD_PARTY_NOTICES.md
