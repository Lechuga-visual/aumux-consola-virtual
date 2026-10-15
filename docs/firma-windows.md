# Identidad y firma de Monitor Mix en Windows

La firma Authenticode identifica al editor y protege la integridad de los archivos.
El certificado relaciona al firmante con una clave pública; la firma de cada
versión se genera con la clave privada. Un sello de tiempo permite verificar que
se firmó mientras el certificado seguía vigente. Un hash SHA-256 del ZIP comprueba
integridad, pero por sí solo no acredita la identidad del editor.

## Opción gratuita para código abierto

[SignPath Foundation](https://signpath.org/) ofrece firma gratuita a proyectos de
código abierto que aprueba. Véase también [SignPath para open source](https://signpath.io/solutions/open-source).
No es una entrega automática de un certificado o una clave privada: el servicio
controla el proceso de firma de las compilaciones autorizadas. Su aceptación y
sus condiciones vigentes deben comprobarse al solicitarlo. Este entorno no pudo
consultar las páginas del programa (HTTP 403), por lo que no se ha verificado aquí
el formulario ni los requisitos actuales.

Para preparar la solicitud de Monitor Mix:

1. Identificar al mantenedor y acreditar control de `Lechuga-visual/audio-obs-asio`.
2. Presentar el repositorio público, la licencia y los avisos de terceros.
3. Explicar cómo se construyen EXE, DLL e instaladores desde un commit concreto,
   con dependencias fijadas y sin secretos dentro del repositorio.
4. Acordar con el servicio qué compilaciones y quiénes pueden solicitar/aprobar
   la firma. Si exige integración con CI, hay que incorporarla y verificarla.
5. Distribuir los archivos firmados devueltos por el servicio, verificar las
   firmas y generar sus nuevos hashes y el ZIP final.

La identidad mostrada en el certificado depende del programa de firma; no se debe
prometer que mostrará el nombre personal del mantenedor. No se ha solicitado alta,
registrado una identidad ni firmado una versión de este proyecto.

## Pruebas internas sin pagar

Un certificado autofirmado permite probar el mecanismo. Solo será de confianza en
los equipos donde se instale expresamente su certificado de confianza. No ofrece
confianza pública para distribuir a cualquier usuario de Windows. No es necesario
ni recomendable desactivar el antivirus o SmartScreen para trabajar en la firma.

La firma no garantiza que desaparezcan avisos: SmartScreen también evalúa
reputación y otros factores; un antivirus puede detectar o marcar archivos
firmados. Si hay una detección falsa concreta se revisa y se presenta el archivo
al proveedor correspondiente.

## Firmar con un certificado disponible

`tools/sign-windows.ps1` prepara un flujo para Windows con un certificado de firma
instalado en `Cert:\CurrentUser\My` y acceso a su clave (incluido un proveedor
hardware compatible). Requiere `signtool.exe` del Windows SDK. No recibe claves
privadas ni contraseñas por argumentos.

```powershell
.\source\tools\sign-windows.ps1 -PackageDirectory C:\MonitorMix-0.10.0-Windows-x64 -CertificateThumbprint HUELLA_DEL_CERTIFICADO
```

Firma y verifica el EXE y receptor OBS; firma los scripts PowerShell, vuelve a
calcular hashes y crea un ZIP `-signed.zip` aparte. No cambia el ZIP original.
Este script no se ha ejecutado con un certificado real en la nube Linux.
Un servicio como SignPath tiene su propia integración y no necesita necesariamente
este script. Los `.cmd` son lanzadores de texto y no admiten Authenticode como un PE.

Puedes comprobar una descarga en Windows con:

```powershell
Get-AuthenticodeSignature .\MonitorMix.exe | Format-List Status,SignerCertificate,TimeStamperCertificate
Get-AuthenticodeSignature .\obs-plugin\monitor-mix-receiver.dll | Format-List Status,SignerCertificate
```

El paquete generado en la nube sigue **sin firma Authenticode** hasta disponer
de un servicio aprobado o un certificado confiable y realizar este paso.
